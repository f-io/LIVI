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
// The other thing done here is a way to switch peripherals on after boot. Mainline has no
// configfs interface for device-tree overlays any more, so `echo NAME > /sys/firmware/ax520/overlay`
// applies /dtbo/ax520-NAME.dtbo (baked into the initramfs). A block that freezes the bus on its first
// register access then costs a power cycle, not a flash.
//
// And the restart: the SoC resets only through its own watchdog, which no mainline driver knows.

#include <linux/clk.h>
#include <linux/clockchips.h>
#include <linux/clocksource.h>
#include <linux/ctype.h>
#include <linux/delay.h>
#include <linux/init.h>
#include <linux/io.h>
#include <linux/kernel_read_file.h>
#include <linux/kobject.h>
#include <linux/limits.h>
#include <linux/of.h>
#include <linux/of_address.h>
#include <linux/of_clk.h>
#include <linux/printk.h>
#include <linux/reboot.h>
#include <linux/slab.h>
#include <linux/string.h>
#include <linux/vmalloc.h>
#include <asm/mach/arch.h>

#define AX520_SYSCNT_BASE	0x0b600000

/* Writes to the watchdog only take while the key sits in the lock register. */
#define AX520_WDT_LOCK		0x00
#define AX520_WDT_LOAD		0x08
#define AX520_WDT_ENABLE	0x10
#define AX520_WDT_KEY		0x5ada7200

static u64 ax520_cntpct(void)
{
	u32 lo, hi;

	asm volatile("mrrc p15, 0, %0, %1, c14" : "=r" (lo), "=r" (hi));
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
	return sysfs_create_file(k, &overlay_attr.attr);
}
device_initcall(ax520_overlay_init);

static void __iomem *ax520_wdt;
static unsigned long ax520_wdt_hz;

static int __init ax520_wdt_init(void)
{
	struct device_node *np = of_find_compatible_node(NULL, NULL, "axera,ax520-wdt");
	struct clk *clk;

	if (!np)
		return 0;
	clk = of_clk_get(np, 0);
	if (!IS_ERR(clk)) {
		ax520_wdt_hz = clk_get_rate(clk);
		clk_put(clk);
		ax520_wdt = of_iomap(np, 0);
	}
	of_node_put(np);
	if (!ax520_wdt || !ax520_wdt_hz)
		pr_warn("ax520: watchdog not usable, reboot will hang\n");
	return 0;
}
arch_initcall(ax520_wdt_init);

static void ax520_restart(enum reboot_mode mode, const char *cmd)
{
	if (!ax520_wdt || !ax520_wdt_hz)
		return;
	writel(AX520_WDT_KEY, ax520_wdt + AX520_WDT_LOCK);
	writel(0, ax520_wdt + AX520_WDT_ENABLE);
	writel(0, ax520_wdt + AX520_WDT_LOCK);
	writel(AX520_WDT_KEY, ax520_wdt + AX520_WDT_LOCK);
	writel(ax520_wdt_hz / 10, ax520_wdt + AX520_WDT_LOAD);
	/* Cleared before enabling, as the vendor kernel does. What it holds is unknown. */
	writel(0, ax520_wdt + 0x04);
	writel(1, ax520_wdt + AX520_WDT_ENABLE);
	writel(0, ax520_wdt + AX520_WDT_LOCK);
	mdelay(1000);
	pr_emerg("ax520: the watchdog did not reset the SoC\n");
}

static const char *const ax520_dt_match[] = {
	"axera,ax520",
	NULL
};

DT_MACHINE_START(AX520_DT, "AXERA AX520")
	.init_time	= ax520_init_time,
	.restart	= ax520_restart,
	.dt_compat	= ax520_dt_match,
MACHINE_END
