/**
 * sdp_clean.c — Android Auto SDP record manager for BlueZ
 *
 * Does two things and stays running (SDP records are owned by the process):
 *   1. Removes the four core BlueZ SDP records (handles 0x10000-0x10003).
 *      These encode standard 16-bit UUIDs as 128-bit in newer BlueZ versions,
 *      which triggers Android's strict UUID-size check and causes it to reject
 *      the entire SDP response ("invalid length for discovery attribute").
 *   2. Registers a clean AA SDP record containing ONLY the 128-bit AA UUID.
 *      No SPP UUID, no ProfileDescriptorList — mixing UUID sizes breaks Android.
 *
 * Requires:
 *   - libbluetooth-dev  (apt install libbluetooth-dev)
 *   - bluetoothd started with --compat flag (enables /var/run/sdp socket)
 *
 * Build:
 *   gcc -o sdp_clean sdp_clean.c -lbluetooth
 *
 * Run (as root):
 *   sudo ./sdp_clean
 *   # Keep running — Ctrl+C deregisters the record
 */

#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <signal.h>
#include <unistd.h>

#include <bluetooth/bluetooth.h>
#include <bluetooth/sdp.h>
#include <bluetooth/sdp_lib.h>

/* Android Auto service UUID: 4de17a00-52cb-11e6-bdf4-0800200c9a66 */
static const uint8_t AA_UUID128[16] = {
    0x4d, 0xe1, 0x7a, 0x00,
    0x52, 0xcb, 0x11, 0xe6,
    0xbd, 0xf4, 0x08, 0x00,
    0x20, 0x0c, 0x9a, 0x66,
};

static sdp_session_t *g_session = NULL;
static uint32_t       g_handle  = 0;

static void cleanup(int sig) {
    (void)sig;
    if (g_session && g_handle) {
        bdaddr_t local = {{0, 0, 0, 0xff, 0xff, 0xff}};
        sdp_device_record_unregister(g_session, &local, NULL);
        printf("[sdp_clean] SDP record deregistered\n");
    }
    if (g_session) {
        sdp_close(g_session);
    }
    exit(0);
}

static void remove_core_records(sdp_session_t *session) {
    /* BlueZ 5.x encodes standard 16-bit UUIDs (PnP/GAP/GATT/DevInfo) as 128-bit
     * in core SDP records (handles 0x10000-0x10003). Android's strict UUID-size
     * check in sdpu_compare_uuid_with_attr() rejects the entire SDP response when
     * it encounters these mixed-size records. We remove them.
     * Fallback: sdp_record_unregister if sdp_device_record_unregister_binary fails.
     */
    bdaddr_t local = {{0, 0, 0, 0xff, 0xff, 0xff}};

    for (uint32_t h = 0x10000; h <= 0x10003; h++) {
        if (sdp_device_record_unregister_binary(session, &local, h) == 0) {
            printf("[sdp_clean] removed core record 0x%05x\n", h);
        } else {
            /* Fallback for BlueZ versions where binary unregister is unsupported */
            sdp_record_t stub;
            memset(&stub, 0, sizeof(stub));
            stub.handle = h;
            if (sdp_record_unregister(session, &stub) == 0) {
                printf("[sdp_clean] removed core record 0x%05x (fallback)\n", h);
            } else {
                printf("[sdp_clean] core record 0x%05x not found or already removed\n", h);
            }
        }
    }
}

static sdp_record_t *build_aa_record(void) {
    sdp_record_t *record = sdp_record_alloc();
    if (!record) { fprintf(stderr, "[sdp_clean] sdp_record_alloc failed\n"); return NULL; }

    /* ── ServiceClassIDList: AA UUID only (128-bit) ── */
    uuid_t aa_uuid;
    sdp_uuid128_create(&aa_uuid, AA_UUID128);
    sdp_list_t *cls = sdp_list_append(NULL, &aa_uuid);
    sdp_set_service_classes(record, cls);
    sdp_list_free(cls, NULL);

    /* ── ProtocolDescriptorList: L2CAP + RFCOMM ch 8 ──
     *
     * sdp_set_access_protos() expects a list-of-lists:
     *   access_proto_list  →  [ proto_list ]
     *   proto_list         →  [ l2cap_list, rfcomm_list ]
     *   l2cap_list         →  [ &l2cap_uuid ]
     *   rfcomm_list        →  [ &rfcomm_uuid, channel ]
     *
     * Passing proto_list directly (without the outer wrapper) causes
     * sdp_set_access_protos to dereference l2cap_uuid as sdp_list_t* → SIGSEGV.
     */
    uuid_t l2cap_uuid, rfcomm_uuid;
    sdp_uuid16_create(&l2cap_uuid,  L2CAP_UUID);
    sdp_uuid16_create(&rfcomm_uuid, RFCOMM_UUID);

    uint8_t ch = 8;
    sdp_data_t *channel = sdp_data_alloc(SDP_UINT8, &ch);

    sdp_list_t *l2cap_list  = sdp_list_append(NULL, &l2cap_uuid);
    sdp_list_t *rfcomm_list = sdp_list_append(NULL, &rfcomm_uuid);
    rfcomm_list = sdp_list_append(rfcomm_list, channel);

    sdp_list_t *proto_list        = sdp_list_append(NULL, l2cap_list);
    proto_list                    = sdp_list_append(proto_list, rfcomm_list);
    /* sdp_set_access_protos expects a list-of-protocol-sequences, not the sequence itself */
    sdp_list_t *access_proto_list = sdp_list_append(NULL, proto_list);
    sdp_set_access_protos(record, access_proto_list);

    sdp_data_free(channel);
    sdp_list_free(access_proto_list, NULL);
    sdp_list_free(proto_list,        NULL);
    sdp_list_free(l2cap_list,        NULL);
    sdp_list_free(rfcomm_list,       NULL);

    /* ── BrowseGroupList: Public Browse Root ── */
    uuid_t browse_uuid;
    sdp_uuid16_create(&browse_uuid, PUBLIC_BROWSE_GROUP);
    sdp_list_t *browse_list = sdp_list_append(NULL, &browse_uuid);
    sdp_set_browse_groups(record, browse_list);
    sdp_list_free(browse_list, NULL);

    /* ── ServiceName ── */
    sdp_set_info_attr(record, "Android Auto Wireless", "LIVI", "");

    printf("[sdp_clean] build_aa_record: done\n"); fflush(stdout);
    return record;
}

int main(void) {
    signal(SIGINT,  cleanup);
    signal(SIGTERM, cleanup);

    bdaddr_t any   = {{0}};
    bdaddr_t local = {{0, 0, 0, 0xff, 0xff, 0xff}};

    /* Connect to local SDP server (requires --compat -P * on bluetoothd) */
    g_session = sdp_connect(&any, &local, SDP_RETRY_IF_BUSY);
    if (!g_session) {
        perror("[sdp_clean] sdp_connect failed");
        fprintf(stderr, "  Is bluetoothd running with --compat?\n");
        return 1;
    }
    printf("[sdp_clean] connected to SDP server\n"); fflush(stdout);

    /* Remove core BlueZ records that confuse Android's UUID size check */
    remove_core_records(g_session);

    /* Register the clean AA SDP record */
    sdp_record_t *record = build_aa_record();
    if (!record) {
        fprintf(stderr, "[sdp_clean] failed to build AA SDP record\n");
        sdp_close(g_session);
        return 1;
    }

    if (sdp_record_register(g_session, record, 0) < 0) {
        perror("[sdp_clean] sdp_record_register failed");
        sdp_record_free(record);
        sdp_close(g_session);
        return 1;
    }

    g_handle = record->handle;
    sdp_record_free(record);  /* session owns it now — don't close session! */
    printf("[sdp_clean] registered AA SDP record (handle=0x%08x)\n", g_handle);
    printf("[sdp_clean] running — Ctrl+C to deregister\n");

    /* Stay alive — SDP record is owned by this process */
    while (1) {
        sleep(10);
    }

    return 0;
}
