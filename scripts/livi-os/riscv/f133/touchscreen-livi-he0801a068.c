// SPDX-License-Identifier: GPL-2.0
/*
 * Touch controller inside the HE0801A068 panel of the XFH F133-B head unit.
 * It speaks the AXS15231 protocol: a read command carrying the frame length,
 * then gesture, point count and 6 bytes per point (event << 6 | x[11:8],
 * x[7:0], id << 4 | y[11:8], y[7:0], weight, area). Its INT line pulses low while
 * a finger is down and stays high otherwise, so a frame is read on each pulse. In
 * case no pulse carries the up event, a point still down is read again every
 * HE_TS_POLL_MS until it is released. Without an interrupt the controller
 * is polled. An idle controller returns 0xff, now and then followed by stale
 * buffer bytes, which are dropped without touching the reported state. A
 * resting second finger drops out of single frames, so a point without an up
 * event is held for HE_TS_GRACE reads.
 */

#include <linux/delay.h>
#include <linux/gpio/consumer.h>
#include <linux/i2c.h>
#include <linux/input.h>
#include <linux/interrupt.h>
#include <linux/input/mt.h>
#include <linux/input/touchscreen.h>
#include <linux/module.h>
#include <linux/mod_devicetable.h>
#include <linux/mutex.h>
#include <linux/workqueue.h>

#define HE_TS_MAX_POINTS	2
#define HE_TS_POLL_MS		16
#define HE_TS_EVENT_UP		1
#define HE_TS_GRACE		3
#define HE_TS_FRAME_LEN		(2 + 6 * HE_TS_MAX_POINTS)

struct he_ts {
	struct i2c_client *client;
	struct input_dev *input;
	struct touchscreen_properties prop;
	struct delayed_work release;
	struct mutex lock;
	struct {
		bool down;
		unsigned int x, y, missed;
	} pt[HE_TS_MAX_POINTS];
};

static const u8 he_ts_read_cmd[] = {
	0xb5, 0xab, 0xa5, 0x5a, 0x00, 0x00, 0x00, HE_TS_FRAME_LEN, 0x00, 0x00, 0x00
};

/* Whether a point is still down afterwards. */
static bool he_ts_read(struct he_ts *ts)
{
	struct input_dev *input = ts->input;
	u8 buf[HE_TS_FRAME_LEN];
	unsigned int n, i, seen = 0;
	bool down = false;

	if (i2c_master_send(ts->client, he_ts_read_cmd, sizeof(he_ts_read_cmd)) != sizeof(he_ts_read_cmd) ||
	    i2c_master_recv(ts->client, buf, sizeof(buf)) != sizeof(buf))
		goto held;

	if (buf[0] == 0xff && buf[1] == 0xff)
		n = 0;
	else if (buf[0] == 0 && buf[1] <= HE_TS_MAX_POINTS)
		n = buf[1];
	else
		goto held;

	for (i = 0; i < n; i++) {
		const u8 *p = &buf[2 + 6 * i];
		unsigned int x = (p[0] & 0x0f) << 8 | p[1];
		unsigned int y = (p[2] & 0x0f) << 8 | p[3];
		unsigned int id = p[2] >> 4;

		if ((p[0] & 0x30) || id >= HE_TS_MAX_POINTS || x > ts->prop.max_x || y > ts->prop.max_y)
			continue;
		seen |= BIT(id);
		ts->pt[id].down = p[0] >> 6 != HE_TS_EVENT_UP;
		ts->pt[id].x = x;
		ts->pt[id].y = y;
		ts->pt[id].missed = 0;
	}

	for (i = 0; i < HE_TS_MAX_POINTS; i++) {
		if (!(seen & BIT(i)) && ts->pt[i].down && ++ts->pt[i].missed > HE_TS_GRACE)
			ts->pt[i].down = false;
		input_mt_slot(input, i);
		input_mt_report_slot_state(input, MT_TOOL_FINGER, ts->pt[i].down);
		if (ts->pt[i].down)
			touchscreen_report_pos(input, &ts->prop, ts->pt[i].x, ts->pt[i].y, true);
		down |= ts->pt[i].down;
	}
	input_mt_sync_frame(input);
	input_sync(input);
	return down;

held:
	for (i = 0; i < HE_TS_MAX_POINTS; i++)
		down |= ts->pt[i].down;
	return down;
}

static void he_ts_poll(struct input_dev *input)
{
	he_ts_read(input_get_drvdata(input));
}

static void he_ts_release(struct work_struct *work)
{
	struct he_ts *ts = container_of(to_delayed_work(work), struct he_ts, release);

	mutex_lock(&ts->lock);
	if (he_ts_read(ts))
		schedule_delayed_work(&ts->release, msecs_to_jiffies(HE_TS_POLL_MS));
	mutex_unlock(&ts->lock);
}

static irqreturn_t he_ts_irq(int irq, void *data)
{
	struct he_ts *ts = data;

	mutex_lock(&ts->lock);
	if (he_ts_read(ts))
		mod_delayed_work(system_wq, &ts->release, msecs_to_jiffies(HE_TS_POLL_MS));
	mutex_unlock(&ts->lock);
	return IRQ_HANDLED;
}

static void he_ts_cancel(void *data)
{
	cancel_delayed_work_sync(data);
}

static int he_ts_probe(struct i2c_client *client)
{
	struct device *dev = &client->dev;
	struct gpio_desc *reset;
	struct he_ts *ts;
	int ret;

	ts = devm_kzalloc(dev, sizeof(*ts), GFP_KERNEL);
	if (!ts)
		return -ENOMEM;
	ts->client = client;

	reset = devm_gpiod_get_optional(dev, "reset", GPIOD_OUT_HIGH);
	if (IS_ERR(reset))
		return dev_err_probe(dev, PTR_ERR(reset), "reset gpio\n");
	if (reset) {
		usleep_range(10000, 11000);
		gpiod_set_value_cansleep(reset, 0);
		msleep(200);
	}

	ts->input = devm_input_allocate_device(dev);
	if (!ts->input)
		return -ENOMEM;
	ts->input->name = "HE0801A068 touchscreen";
	ts->input->id.bustype = BUS_I2C;
	input_set_drvdata(ts->input, ts);

	input_set_abs_params(ts->input, ABS_MT_POSITION_X, 0, 451, 0, 0);
	input_set_abs_params(ts->input, ABS_MT_POSITION_Y, 0, 1279, 0, 0);
	touchscreen_parse_properties(ts->input, true, &ts->prop);

	ret = input_mt_init_slots(ts->input, HE_TS_MAX_POINTS, INPUT_MT_DIRECT);
	if (ret)
		return ret;
	if (client->irq <= 0) {
		ret = input_setup_polling(ts->input, he_ts_poll);
		if (ret)
			return ret;
		input_set_poll_interval(ts->input, HE_TS_POLL_MS);
		return input_register_device(ts->input);
	}

	mutex_init(&ts->lock);
	INIT_DELAYED_WORK(&ts->release, he_ts_release);
	ret = devm_add_action_or_reset(dev, he_ts_cancel, &ts->release);
	if (ret)
		return ret;
	ret = input_register_device(ts->input);
	if (ret)
		return ret;
	return devm_request_threaded_irq(dev, client->irq, NULL, he_ts_irq, IRQF_ONESHOT,
					 client->name, ts);
}

static const struct of_device_id he_ts_of_match[] = {
	{ .compatible = "livi,he0801a068-ts" },
	{ }
};
MODULE_DEVICE_TABLE(of, he_ts_of_match);

static struct i2c_driver he_ts_driver = {
	.driver = {
		.name = "he0801a068-ts",
		.of_match_table = he_ts_of_match,
	},
	.probe = he_ts_probe,
};
module_i2c_driver(he_ts_driver);

MODULE_DESCRIPTION("HE0801A068 panel touchscreen");
MODULE_LICENSE("GPL");
