// SPDX-License-Identifier: GPL-2.0-only
/*
 * Allwinner V821 CCU: the module clocks, bus gates and resets of the blocks the LIVI Link board uses.
 */

#include <linux/clk-provider.h>
#include <linux/io.h>
#include <linux/module.h>
#include <linux/platform_device.h>

#include "ccu_common.h"
#include "ccu_div.h"
#include "ccu_gate.h"
#include "ccu_mp.h"
#include "ccu_reset.h"

#include <dt-bindings/clock/sun300i-v821-ccu.h>
#include <dt-bindings/reset/sun300i-v821-ccu.h>

static const char * const spif_parents[] = {
	"dcxo", "pll-peri-512m", "pll-peri-384m", "pll-peri-307m",
};
static SUNXI_CCU_MP_WITH_MUX_GATE(spif_clk, "spif", spif_parents, 0x020,
				  0, 4, 16, 2, 24, 2, BIT(31), 0);

static const char * const smhc1_parents[] = {
	"dcxo", "pll-peri-192m", "pll-peri-219m",
};
static SUNXI_CCU_MP_WITH_MUX_GATE(smhc1_clk, "smhc1", smhc1_parents, 0x05c,
				  0, 5, 16, 2, 24, 3, BIT(31), 0);

static const char * const spi1_parents[] = {
	"dcxo", "pll-peri-307m", "pll-peri-236m", "", "pll-peri-48m",
};
static SUNXI_CCU_MP_WITH_MUX_GATE(spi1_clk, "spi1", spi1_parents, 0x064,
				  0, 4, 16, 2, 24, 3, BIT(31), 0);

static SUNXI_CCU_GATE(usb_24m_clk, "usb-24m", "dcxo", 0x07c, BIT(3), 0);
static SUNXI_CCU_GATE(bus_usb_clk, "bus-usb", "ahb", 0x080, BIT(19), 0);
static SUNXI_CCU_GATE(bus_otg_clk, "bus-otg", "ahb", 0x080, BIT(20), 0);
static SUNXI_CCU_GATE(bus_spif_clk, "bus-spif", "ahb", 0x084, BIT(5), 0);
static SUNXI_CCU_GATE(mbus_usb_clk, "mbus-usb", "ahb", 0x084, BIT(14), 0);
static SUNXI_CCU_GATE(mbus_smhc1_clk, "mbus-smhc1", "ahb", 0x084, BIT(16), 0);
static SUNXI_CCU_GATE(bus_spi1_clk, "bus-spi1", "ahb", 0x084, BIT(19), 0);
static SUNXI_CCU_GATE(bus_smhc1_clk, "bus-smhc1", "ahb", 0x084, BIT(21), 0);
static SUNXI_CCU_GATE(bus_twi1_clk, "bus-twi1", "apb-spc", 0x084, BIT(24), 0);

static struct ccu_common *sun300i_v821_ccu_clks[] = {
	&spif_clk.common,
	&smhc1_clk.common,
	&spi1_clk.common,
	&usb_24m_clk.common,
	&bus_usb_clk.common,
	&bus_otg_clk.common,
	&bus_spif_clk.common,
	&mbus_usb_clk.common,
	&mbus_smhc1_clk.common,
	&bus_spi1_clk.common,
	&bus_smhc1_clk.common,
	&bus_twi1_clk.common,
};

static struct clk_hw_onecell_data sun300i_v821_hw_clks = {
	.num	= CLK_MBUS_USB + 1,
	.hws	= {
		[CLK_SPIF]		= &spif_clk.common.hw,
		[CLK_BUS_SPIF]		= &bus_spif_clk.common.hw,
		[CLK_SMHC1]		= &smhc1_clk.common.hw,
		[CLK_BUS_SMHC1]		= &bus_smhc1_clk.common.hw,
		[CLK_MBUS_SMHC1]	= &mbus_smhc1_clk.common.hw,
		[CLK_SPI1]		= &spi1_clk.common.hw,
		[CLK_BUS_SPI1]		= &bus_spi1_clk.common.hw,
		[CLK_BUS_TWI1]		= &bus_twi1_clk.common.hw,
		[CLK_USB_24M]		= &usb_24m_clk.common.hw,
		[CLK_BUS_OTG]		= &bus_otg_clk.common.hw,
		[CLK_BUS_USB]		= &bus_usb_clk.common.hw,
		[CLK_MBUS_USB]		= &mbus_usb_clk.common.hw,
	},
};

static const struct ccu_reset_map sun300i_v821_ccu_resets[] = {
	[RST_BUS_SPIF]	= { 0x094, BIT(5) },
	[RST_BUS_SMHC1]	= { 0x094, BIT(21) },
	[RST_BUS_SPI1]	= { 0x094, BIT(19) },
	[RST_BUS_TWI1]	= { 0x094, BIT(24) },
	[RST_USB_PHY]	= { 0x090, BIT(23) },
	[RST_BUS_OTG]	= { 0x090, BIT(20) },
	[RST_BUS_USB]	= { 0x090, BIT(19) },
};

static const struct sunxi_ccu_desc sun300i_v821_ccu_desc = {
	.ccu_clks	= sun300i_v821_ccu_clks,
	.num_ccu_clks	= ARRAY_SIZE(sun300i_v821_ccu_clks),
	.hw_clks	= &sun300i_v821_hw_clks,
	.resets		= sun300i_v821_ccu_resets,
	.num_resets	= ARRAY_SIZE(sun300i_v821_ccu_resets),
};

static int sun300i_v821_ccu_probe(struct platform_device *pdev)
{
	void __iomem *reg;

	reg = devm_platform_ioremap_resource(pdev, 0);
	if (IS_ERR(reg))
		return PTR_ERR(reg);

	return devm_sunxi_ccu_probe(&pdev->dev, reg, &sun300i_v821_ccu_desc);
}

static const struct of_device_id sun300i_v821_ccu_ids[] = {
	{ .compatible = "allwinner,sun300i-v821-ccu" },
	{ }
};
MODULE_DEVICE_TABLE(of, sun300i_v821_ccu_ids);

static struct platform_driver sun300i_v821_ccu_driver = {
	.probe	= sun300i_v821_ccu_probe,
	.driver	= {
		.name			= "sun300i-v821-ccu",
		.suppress_bind_attrs	= true,
		.of_match_table		= sun300i_v821_ccu_ids,
	},
};
module_platform_driver(sun300i_v821_ccu_driver);

MODULE_IMPORT_NS("SUNXI_CCU");
MODULE_DESCRIPTION("Allwinner V821 CCU");
MODULE_LICENSE("GPL");
