// SPDX-License-Identifier: GPL-2.0
// AXERA AX520: single Cortex-A7, a real ARM GIC, and otherwise stock Synopsys DesignWare IP,
// all handled by the generic ARM multiplatform boot path and standard DT-probed drivers.
//
// The one thing done here is the time source. On the first boots the architected counter did not
// count: every initcall took "0 usecs" and the jitter entropy self-test never finished. So before the
// timers are probed the counter is checked, and if it stands still:
//  1. the DT lists a syscounter block at 0x0b600000; if its first register has bit 0 clear, that bit
//     is set, as CNTCR.EN of an ARM system counter would be, and the counter is checked again,
//  2. if it still does not count, the ARM timer is switched off and the two DW APB timers from the
//     DT (disabled otherwise) take over tick, clock, sched_clock and udelay.
// Everything is printed, so the boot log says which case it was.
//
// The other thing done here is a way to switch peripherals on after boot. Kernel 6.18 has no
// configfs interface for device-tree overlays any more, so `echo NAME > /sys/firmware/ax520/overlay`
// applies /dtbo/ax520-NAME.dtbo (baked into the initramfs). A block that freezes the bus on its first
// register access then costs a power cycle, not a flash.

#include <linux/clockchips.h>
#include <linux/clocksource.h>
#include <linux/ctype.h>
#include <linux/init.h>
#include <linux/io.h>
#include <linux/kernel_read_file.h>
#include <linux/kobject.h>
#include <linux/jiffies.h>
#include <linux/kernel_stat.h>
#include <linux/limits.h>
#include <linux/of.h>
#include <linux/of_clk.h>
#include <linux/printk.h>
#include <linux/slab.h>
#include <linux/string.h>
#include <linux/sysrq.h>
#include <linux/vmalloc.h>
#include <asm/mach/arch.h>

#define AX520_SYSCNT_BASE	0x0b600000

static u64 ax520_cntpct(void)
{
	u32 lo, hi;

	asm volatile("mrrc p15, 0, %0, %1, c14" : "=r" (lo), "=r" (hi));
	return ((u64)hi << 32) | lo;
}

static u64 ax520_cntvct(void)
{
	u32 lo, hi;

	asm volatile("mrrc p15, 1, %0, %1, c14" : "=r" (lo), "=r" (hi));
	return ((u64)hi << 32) | lo;
}

static bool __init ax520_counter_runs(void)
{
	u64 p0 = ax520_cntpct();
	unsigned long i;

	for (i = 0; i < 3000000; i++)
		asm volatile("" ::: "memory");
	return ax520_cntpct() != p0;
}

static void __init ax520_set_status(const char *compatible, const char *status)
{
	struct device_node *np;

	for_each_compatible_node(np, NULL, compatible) {
		struct property *p = kzalloc(sizeof(*p), GFP_KERNEL);

		if (!p)
			return;
		p->name = kstrdup("status", GFP_KERNEL);
		p->value = kstrdup(status, GFP_KERNEL);
		p->length = strlen(status) + 1;
		if (of_update_property(np, p))
			pr_warn("ax520: could not set status %s on %pOF\n", status, np);
	}
}

static bool __init ax520_try_syscounter(void)
{
	void __iomem *cnt = ioremap(AX520_SYSCNT_BASE, 0x1000);
	bool runs;

	if (!cnt) {
		pr_warn("ax520 syscnt: ioremap failed\n");
		return false;
	}
	if (readl(cnt) & 1) {
		iounmap(cnt);
		return false;
	}
	writel(readl(cnt) | 1, cnt);
	iounmap(cnt);
	runs = ax520_counter_runs();
	pr_warn("ax520: the bootloader left the ARM counter stopped, enabled it in the syscounter block (%s)\n",
		runs ? "counts now" : "STILL STOPPED");
	return runs;
}

static void __init ax520_init_time(void)
{
	if (!ax520_counter_runs() && !ax520_try_syscounter()) {
		pr_warn("ax520: the ARM counter does not count, using the DW APB timers\n");
		ax520_set_status("arm,armv7-timer", "disabled");
		ax520_set_status("snps,dw-apb-timer", "okay");
	}
	of_clk_init(NULL);
	timer_probe();
	tick_setup_hrtimer_broadcast();
}

/*
 * SysRq y: state of the timer, the GIC, the USB core and the IRQ counters, printed from the UART interrupt.
 * A kernel whose tick has died still takes that interrupt, and no task has to run for this.
 */
static void __iomem *dbg_gicd, *dbg_gicc, *dbg_usb, *dbg_cnt;

static void ax520_sysrq_dump(u8 key)
{
	u32 pctl, vctl, frq, lo, hi;
	int i;

	asm volatile("mrc p15, 0, %0, c14, c2, 1" : "=r" (pctl));
	asm volatile("mrc p15, 0, %0, c14, c3, 1" : "=r" (vctl));
	asm volatile("mrc p15, 0, %0, c14, c0, 0" : "=r" (frq));
	pr_info("ax520 dump: jiffies %lu, cntpct %llu, cntvct %llu, cntfrq %u, cntp_ctl %08x, cntv_ctl %08x\n",
		jiffies, ax520_cntpct(), ax520_cntvct(), frq, pctl, vctl);
	asm volatile("mrrc p15, 2, %0, %1, c14" : "=r" (lo), "=r" (hi));
	pr_info("ax520 dump: cntp_cval %08x%08x\n", hi, lo);
	if (dbg_gicd && dbg_gicc)
		pr_info("ax520 dump: gic enable %08x %08x, pending %08x %08x, active %08x %08x, cpu ctl %08x pmr %08x rpr %08x hppir %08x\n",
			readl(dbg_gicd + 0x100), readl(dbg_gicd + 0x104), readl(dbg_gicd + 0x200), readl(dbg_gicd + 0x204),
			readl(dbg_gicd + 0x300), readl(dbg_gicd + 0x304), readl(dbg_gicc), readl(dbg_gicc + 4),
			readl(dbg_gicc + 0x14), readl(dbg_gicc + 0x18));
	if (dbg_usb)
		pr_info("ax520 dump: usb gisr %08x gmir %08x dmcr %08x phytmsr %08x dmigr %08x digr %08x disgr0 %08x disgr1 %08x disgr2 %08x otgisr %08x\n",
			readl(dbg_usb + 0xc0), readl(dbg_usb + 0xc4), readl(dbg_usb + 0x100), readl(dbg_usb + 0x114),
			readl(dbg_usb + 0x130), readl(dbg_usb + 0x140), readl(dbg_usb + 0x144), readl(dbg_usb + 0x148),
			readl(dbg_usb + 0x14c), readl(dbg_usb + 0x84));
	if (dbg_cnt)
		pr_info("ax520 dump: syscnt cntcr %08x cntsr %08x\n", readl(dbg_cnt), readl(dbg_cnt + 4));
	for (i = 1; i < 64; i++)
		if (kstat_irqs_usr(i))
			pr_info("ax520 dump: irq %d count %u\n", i, kstat_irqs_usr(i));
}

static const struct sysrq_key_op ax520_sysrq_op = {
	.handler	= ax520_sysrq_dump,
	.help_msg	= "ax520dump(y)",
	.action_msg	= "AX520 dump",
};

static ssize_t overlay_store(struct kobject *kobj, struct kobj_attribute *attr,
			     const char *buf, size_t count)
{
	char path[64];
	void *fdt = NULL;
	size_t size = 0;
	int id = 0, ret, i;
	ssize_t rd;

	for (i = 0; i < count && buf[i] != '\n'; i++)
		if (!isalnum(buf[i]) && buf[i] != '-')
			return -EINVAL;
	if (!i || i > 32)
		return -EINVAL;
	snprintf(path, sizeof(path), "/dtbo/ax520-%.*s.dtbo", i, buf);

	rd = kernel_read_file_from_path(path, 0, &fdt, INT_MAX, &size, READING_UNKNOWN);
	if (rd < 0) {
		pr_err("ax520 overlay: cannot read %s: %zd\n", path, rd);
		return rd;
	}
	pr_info("ax520 overlay: applying %s (%zu B)\n", path, size);
	ret = of_overlay_fdt_apply(fdt, size, &id, NULL);
	vfree(fdt);
	if (ret) {
		pr_err("ax520 overlay: %s failed: %d\n", path, ret);
		return ret;
	}
	pr_info("ax520 overlay: %s applied, id %d\n", path, id);
	return count;
}

static struct kobj_attribute overlay_attr = __ATTR_WO(overlay);

static int __init ax520_overlay_init(void)
{
	struct kobject *k = kobject_create_and_add("ax520", firmware_kobj);

	if (!k)
		return -ENOMEM;
	dbg_gicd = ioremap(0x08f01000, 0x1000);
	dbg_gicc = ioremap(0x08f02000, 0x1000);
	dbg_usb = ioremap(0x0b500000, 0x200);
	dbg_cnt = ioremap(AX520_SYSCNT_BASE, 0x1000);
	register_sysrq_key('y', &ax520_sysrq_op);
	return sysfs_create_file(k, &overlay_attr.attr);
}
device_initcall(ax520_overlay_init);

static const char *const ax520_dt_match[] = {
	"axera,ax520",
	NULL
};

DT_MACHINE_START(AX520_DT, "AXERA AX520")
	.init_time	= ax520_init_time,
	.dt_compat	= ax520_dt_match,
MACHINE_END
