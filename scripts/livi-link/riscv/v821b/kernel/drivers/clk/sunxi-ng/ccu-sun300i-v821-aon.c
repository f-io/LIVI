// SPDX-License-Identifier: GPL-2.0-only
/*
 * Allwinner V821 AON CCU: the peripheral PLL and the bus clocks the other blocks hang off. boot0 sets
 * them up, Linux only reads them.
 */

#include <linux/bitfield.h>
#include <linux/clk-provider.h>
#include <linux/io.h>
#include <linux/math64.h>
#include <linux/module.h>
#include <linux/platform_device.h>

#include "ccu_common.h"
#include "ccu_div.h"

#include <dt-bindings/clock/sun300i-v821-aon-ccu.h>

#define PLL_PERI_N		GENMASK(15, 8)

/* The VCO runs at DCXO * N * 2 / 5, 3072 MHz from the 40 MHz DCXO. */
static unsigned long pll_peri_recalc_rate(struct clk_hw *hw, unsigned long parent_rate)
{
	struct ccu_common *cm = hw_to_ccu_common(hw);
	u32 n = FIELD_GET(PLL_PERI_N, readl(cm->base + cm->reg)) + 1;

	return div_u64((u64)parent_rate * n * 2, 5);
}

static const struct clk_ops pll_peri_ops = {
	.recalc_rate	= pll_peri_recalc_rate,
};

static struct ccu_common pll_peri_clk = {
	.reg		= 0x020,
	.hw.init	= CLK_HW_INIT("pll-peri", "dcxo", &pll_peri_ops, 0),
};

static CLK_FIXED_FACTOR_HW(pll_peri_768m_clk, "pll-peri-768m", &pll_peri_clk.hw, 4, 1, 0);
static CLK_FIXED_FACTOR_HW(pll_peri_512m_clk, "pll-peri-512m", &pll_peri_clk.hw, 6, 1, 0);
static CLK_FIXED_FACTOR_HW(pll_peri_384m_clk, "pll-peri-384m", &pll_peri_clk.hw, 8, 1, 0);
static CLK_FIXED_FACTOR_HW(pll_peri_307m_clk, "pll-peri-307m", &pll_peri_clk.hw, 10, 1, 0);
static CLK_FIXED_FACTOR_HW(pll_peri_236m_clk, "pll-peri-236m", &pll_peri_clk.hw, 13, 1, 0);
static CLK_FIXED_FACTOR_HW(pll_peri_219m_clk, "pll-peri-219m", &pll_peri_clk.hw, 14, 1, 0);
static CLK_FIXED_FACTOR_HW(pll_peri_192m_clk, "pll-peri-192m", &pll_peri_clk.hw, 16, 1, 0);
static CLK_FIXED_FACTOR_HW(pll_peri_48m_clk, "pll-peri-48m", &pll_peri_clk.hw, 64, 1, 0);

static const char * const ahb_parents[] = { "dcxo", "pll-peri-768m", "rc-1m" };
static SUNXI_CCU_M_WITH_MUX(ahb_clk, "ahb", ahb_parents, 0x500, 0, 5, 24, 2, CLK_IS_CRITICAL);

static const char * const apb_parents[] = { "dcxo", "pll-peri-384m", "rc-1m" };
static SUNXI_CCU_M_WITH_MUX(apb_clk, "apb", apb_parents, 0x504, 0, 5, 24, 2, CLK_IS_CRITICAL);

static const char * const apb_spc_parents[] = { "dcxo", "", "rc-1m", "pll-peri-192m" };
static SUNXI_CCU_M_WITH_MUX(apb_spc_clk, "apb-spc", apb_spc_parents, 0x580, 0, 5, 24, 2,
			    CLK_IS_CRITICAL);

static struct ccu_common *sun300i_v821_aon_ccu_clks[] = {
	&pll_peri_clk,
	&ahb_clk.common,
	&apb_clk.common,
	&apb_spc_clk.common,
};

static struct clk_hw_onecell_data sun300i_v821_aon_hw_clks = {
	.num	= CLK_AON_APB_SPC + 1,
	.hws	= {
		[CLK_AON_PLL_PERI]	= &pll_peri_clk.hw,
		[CLK_AON_PLL_PERI_768M]	= &pll_peri_768m_clk.hw,
		[CLK_AON_PLL_PERI_512M]	= &pll_peri_512m_clk.hw,
		[CLK_AON_PLL_PERI_384M]	= &pll_peri_384m_clk.hw,
		[CLK_AON_PLL_PERI_307M]	= &pll_peri_307m_clk.hw,
		[CLK_AON_PLL_PERI_236M]	= &pll_peri_236m_clk.hw,
		[CLK_AON_PLL_PERI_219M]	= &pll_peri_219m_clk.hw,
		[CLK_AON_PLL_PERI_192M]	= &pll_peri_192m_clk.hw,
		[CLK_AON_PLL_PERI_48M]	= &pll_peri_48m_clk.hw,
		[CLK_AON_AHB]		= &ahb_clk.common.hw,
		[CLK_AON_APB]		= &apb_clk.common.hw,
		[CLK_AON_APB_SPC]	= &apb_spc_clk.common.hw,
	},
};

static const struct sunxi_ccu_desc sun300i_v821_aon_ccu_desc = {
	.ccu_clks	= sun300i_v821_aon_ccu_clks,
	.num_ccu_clks	= ARRAY_SIZE(sun300i_v821_aon_ccu_clks),
	.hw_clks	= &sun300i_v821_aon_hw_clks,
};

static int sun300i_v821_aon_ccu_probe(struct platform_device *pdev)
{
	void __iomem *reg;

	reg = devm_platform_ioremap_resource(pdev, 0);
	if (IS_ERR(reg))
		return PTR_ERR(reg);

	return devm_sunxi_ccu_probe(&pdev->dev, reg, &sun300i_v821_aon_ccu_desc);
}

static const struct of_device_id sun300i_v821_aon_ccu_ids[] = {
	{ .compatible = "allwinner,sun300i-v821-aon-ccu" },
	{ }
};
MODULE_DEVICE_TABLE(of, sun300i_v821_aon_ccu_ids);

static struct platform_driver sun300i_v821_aon_ccu_driver = {
	.probe	= sun300i_v821_aon_ccu_probe,
	.driver	= {
		.name			= "sun300i-v821-aon-ccu",
		.suppress_bind_attrs	= true,
		.of_match_table		= sun300i_v821_aon_ccu_ids,
	},
};
module_platform_driver(sun300i_v821_aon_ccu_driver);

MODULE_IMPORT_NS("SUNXI_CCU");
MODULE_DESCRIPTION("Allwinner V821 AON CCU");
MODULE_LICENSE("GPL");
