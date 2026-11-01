// SPDX-License-Identifier: GPL-2.0-only
/*
 * Allwinner V821 SPIF, the controller of the boot flash. An operation with data runs as one DMA
 * descriptor through a bounce buffer, one without data straight from the registers. Register layout
 * and descriptor format are those of the vendor driver.
 */

#include <linux/bitfield.h>
#include <linux/clk.h>
#include <linux/completion.h>
#include <linux/dma-mapping.h>
#include <linux/interrupt.h>
#include <linux/io.h>
#include <linux/iopoll.h>
#include <linux/log2.h>
#include <linux/module.h>
#include <linux/of.h>
#include <linux/platform_device.h>
#include <linux/reset.h>
#include <linux/spi/spi.h>
#include <linux/spi/spi-mem.h>

#define SPIF_GC			0x04
#define  SPIF_GC_DMA_MODE	BIT(0)
#define  SPIF_GC_NMODE_EN	BIT(2)
#define  SPIF_GC_CPHA		BIT(4)
#define  SPIF_GC_CPOL		BIT(5)
#define  SPIF_GC_SS		GENMASK(7, 6)
#define  SPIF_GC_CS_POL		BIT(8)
#define  SPIF_GC_HOLD_EN	BIT(13)
#define  SPIF_GC_WP_EN		BIT(15)
#define  SPIF_GC_DTR_EN		BIT(16)
#define SPIF_GCA		0x08
#define  SPIF_GCA_RF_RST	BIT(0)
#define  SPIF_GCA_WF_RST	BIT(1)
#define  SPIF_GCA_RESET		BIT(3)
#define SPIF_TC			0x0c
#define  SPIF_TC_SCKOUT_SRC	BIT(26)
#define SPIF_INT_EN		0x14
#define SPIF_INT_STA		0x18
#define  SPIF_INT_RF_OVF	BIT(8)
#define  SPIF_INT_RF_UDF	BIT(9)
#define  SPIF_INT_WF_OVF	BIT(10)
#define  SPIF_INT_DMA_DONE	BIT(24)
#define  SPIF_INT_ERR		(SPIF_INT_RF_OVF | SPIF_INT_RF_UDF | SPIF_INT_WF_OVF)
#define SPIF_CSD		0x1c
#define  SPIF_CSD_START		GENMASK(7, 0)
#define  SPIF_CSD_END		GENMASK(15, 8)
#define  SPIF_CSD_DEASSERT	GENMASK(23, 16)
#define SPIF_PHC		0x20
#define SPIF_TCF		0x24
#define SPIF_TCS		0x28
#define SPIF_TNM		0x2c
#define SPIF_DMA_CTL		0x40
#define  SPIF_DMA_START		BIT(0)
#define  SPIF_DMA_DESC_LEN	GENMASK(11, 4)
#define SPIF_DSC		0x44
#define SPIF_CFT		0x4c
#define  SPIF_CFT_RF_EMPTY	GENMASK(7, 0)
#define  SPIF_CFT_RF_FULL	GENMASK(15, 8)
#define  SPIF_CFT_WF_EMPTY	GENMASK(23, 16)
#define  SPIF_CFT_WF_FULL	GENMASK(31, 24)

/* The phases of a transfer, in PHC and in descriptor word 4 */
#define SPIF_PH_RX		BIT(8)
#define SPIF_PH_TX		BIT(12)
#define SPIF_PH_DUMMY		BIT(16)
#define SPIF_PH_ADDR		BIT(24)
#define SPIF_PH_CMD		BIT(28)

/* Opcode and bus widths (log2 of the lines), in TCS and in descriptor word 6 */
#define SPIF_TCS_DATA_W		GENMASK(1, 0)
#define SPIF_TCS_ADDR_W		GENMASK(5, 4)
#define SPIF_TCS_CMD_W		GENMASK(9, 8)
#define SPIF_TCS_OPCODE		GENMASK(31, 24)

/* Counts (data in bytes, dummy in clock cycles), in TNM and in descriptor word 7 */
#define SPIF_TNM_DATA		GENMASK(15, 0)
#define SPIF_TNM_DUMMY		GENMASK(23, 16)
#define SPIF_TNM_ADDR_BYTES	GENMASK(25, 24)
#define SPIF_TNM_NORMAL		BIT(28)

/* Descriptor words 0 and 1 */
#define SPIF_DESC_LAST		BIT(0)
#define SPIF_DESC_TO_DRAM	BIT(1)
#define SPIF_DESC_BURST		GENMASK(6, 4)
#define  SPIF_DESC_BURST_INCR16	7
#define SPIF_DESC_BLOCK		GENMASK(31, 24)
#define  SPIF_DESC_BLOCK_64B	3

#define SPIF_MAX_DATA		4096
#define SPIF_TIMEOUT_US		(100 * USEC_PER_MSEC)

struct sun300i_spif_desc {
	__le32 flags;
	__le32 len;
	__le32 data;
	__le32 next;
	__le32 phase;
	__le32 addr;
	__le32 bus;
	__le32 count;
};

struct sun300i_spif {
	void __iomem *base;
	struct clk *mod_clk;
	struct completion done;
	u32 status;
	struct sun300i_spif_desc *desc;
	dma_addr_t desc_dma;
	void *buf;
	dma_addr_t buf_dma;
};

static irqreturn_t sun300i_spif_irq(int irq, void *data)
{
	struct sun300i_spif *spif = data;
	u32 sta = readl(spif->base + SPIF_INT_STA);

	if (!(sta & (SPIF_INT_DMA_DONE | SPIF_INT_ERR)))
		return IRQ_NONE;

	writel(sta, spif->base + SPIF_INT_STA);
	spif->status = sta;
	complete(&spif->done);

	return IRQ_HANDLED;
}

static int sun300i_spif_wait_idle(struct sun300i_spif *spif)
{
	u32 val;

	return readl_poll_timeout(spif->base + SPIF_GC, val, !(val & SPIF_GC_NMODE_EN), 1,
				  SPIF_TIMEOUT_US);
}

static int sun300i_spif_run_regs(struct sun300i_spif *spif, u32 phase, u32 addr, u32 bus,
				 u32 count)
{
	u32 gc = readl(spif->base + SPIF_GC) & ~SPIF_GC_DMA_MODE;

	writel(gc, spif->base + SPIF_GC);
	writel(phase, spif->base + SPIF_PHC);
	writel(addr, spif->base + SPIF_TCF);
	writel(bus, spif->base + SPIF_TCS);
	writel(count, spif->base + SPIF_TNM);
	writel(gc | SPIF_GC_NMODE_EN, spif->base + SPIF_GC);

	return sun300i_spif_wait_idle(spif);
}

static int sun300i_spif_run_dma(struct sun300i_spif *spif, const struct spi_mem_op *op, u32 phase,
				u32 bus, u32 count)
{
	bool in = op->data.dir == SPI_MEM_DATA_IN;
	struct sun300i_spif_desc *desc = spif->desc;
	int ret;

	desc->flags = cpu_to_le32(SPIF_DESC_LAST | (in ? SPIF_DESC_TO_DRAM : 0) |
				  FIELD_PREP(SPIF_DESC_BURST, SPIF_DESC_BURST_INCR16));
	desc->len = cpu_to_le32(FIELD_PREP(SPIF_DESC_BLOCK, SPIF_DESC_BLOCK_64B) | op->data.nbytes);
	desc->data = cpu_to_le32(spif->buf_dma >> 2);
	desc->next = 0;
	desc->phase = cpu_to_le32(phase);
	desc->addr = cpu_to_le32(op->addr.val);
	desc->bus = cpu_to_le32(bus);
	desc->count = cpu_to_le32(count);

	if (!in)
		memcpy(spif->buf, op->data.buf.out, op->data.nbytes);

	reinit_completion(&spif->done);
	spif->status = 0;
	writel(readl(spif->base + SPIF_GCA) | SPIF_GCA_RF_RST | SPIF_GCA_WF_RST,
	       spif->base + SPIF_GCA);
	writel(~0, spif->base + SPIF_INT_STA);
	writel(SPIF_INT_DMA_DONE | SPIF_INT_ERR, spif->base + SPIF_INT_EN);
	writel(readl(spif->base + SPIF_GC) | SPIF_GC_DMA_MODE, spif->base + SPIF_GC);
	writel(spif->desc_dma >> 2, spif->base + SPIF_DSC);
	writel(FIELD_PREP(SPIF_DMA_DESC_LEN, sizeof(*desc)) | SPIF_DMA_START,
	       spif->base + SPIF_DMA_CTL);

	ret = wait_for_completion_timeout(&spif->done, usecs_to_jiffies(SPIF_TIMEOUT_US)) ?
	      0 : -ETIMEDOUT;
	writel(0, spif->base + SPIF_INT_EN);
	if (ret)
		return ret;
	if (spif->status & SPIF_INT_ERR)
		return -EIO;

	/* The DMA is done once the data sits in the FIFO, the flash may still be clocking it out. */
	if (!in)
		return sun300i_spif_wait_idle(spif);

	memcpy(op->data.buf.in, spif->buf, op->data.nbytes);
	return 0;
}

static int sun300i_spif_exec_op(struct spi_mem *mem, const struct spi_mem_op *op)
{
	struct sun300i_spif *spif = spi_controller_get_devdata(mem->spi->controller);
	u32 phase = SPIF_PH_CMD, count = SPIF_TNM_NORMAL, bus;

	bus = FIELD_PREP(SPIF_TCS_OPCODE, op->cmd.opcode) |
	      FIELD_PREP(SPIF_TCS_CMD_W, ilog2(op->cmd.buswidth));

	if (op->addr.nbytes) {
		phase |= SPIF_PH_ADDR;
		bus |= FIELD_PREP(SPIF_TCS_ADDR_W, ilog2(op->addr.buswidth));
		count |= FIELD_PREP(SPIF_TNM_ADDR_BYTES, op->addr.nbytes - 1);
	}

	if (op->dummy.nbytes) {
		phase |= SPIF_PH_DUMMY;
		count |= FIELD_PREP(SPIF_TNM_DUMMY, op->dummy.nbytes * 8 / op->dummy.buswidth);
	}

	if (!op->data.nbytes)
		return sun300i_spif_run_regs(spif, phase, op->addr.val, bus, count);

	phase |= op->data.dir == SPI_MEM_DATA_IN ? SPIF_PH_RX : SPIF_PH_TX;
	bus |= FIELD_PREP(SPIF_TCS_DATA_W, ilog2(op->data.buswidth));
	count |= FIELD_PREP(SPIF_TNM_DATA, op->data.nbytes);

	return sun300i_spif_run_dma(spif, op, phase, bus, count);
}

static bool sun300i_spif_supports_op(struct spi_mem *mem, const struct spi_mem_op *op)
{
	if (!spi_mem_default_supports_op(mem, op))
		return false;

	if (op->cmd.nbytes != 1 || op->cmd.buswidth != 1 || op->addr.nbytes > 4)
		return false;

	/* Dual and quad I/O reads carry mode bits in their dummy phase, keep to the others. */
	return !op->dummy.nbytes || op->dummy.buswidth == 1;
}

static int sun300i_spif_adjust_op_size(struct spi_mem *mem, struct spi_mem_op *op)
{
	op->data.nbytes = min_t(unsigned int, op->data.nbytes, SPIF_MAX_DATA);
	return 0;
}

static const struct spi_controller_mem_ops sun300i_spif_mem_ops = {
	.adjust_op_size	= sun300i_spif_adjust_op_size,
	.supports_op	= sun300i_spif_supports_op,
	.exec_op	= sun300i_spif_exec_op,
};

static int sun300i_spif_setup(struct spi_device *spi)
{
	struct sun300i_spif *spif = spi_controller_get_devdata(spi->controller);

	return clk_set_rate(spif->mod_clk, spi->max_speed_hz);
}

static int sun300i_spif_hw_init(struct sun300i_spif *spif)
{
	/* The boot loader calibrates the read sample point on every boot. */
	u32 tc = readl(spif->base + SPIF_TC);
	u32 val;
	int ret;

	writel(SPIF_GCA_RESET, spif->base + SPIF_GCA);
	ret = readl_poll_timeout(spif->base + SPIF_GCA, val, !(val & SPIF_GCA_RESET), 1,
				 SPIF_TIMEOUT_US);
	if (ret)
		return ret;

	writel(tc & ~SPIF_TC_SCKOUT_SRC, spif->base + SPIF_TC);

	val = readl(spif->base + SPIF_GC);
	val &= ~(SPIF_GC_DMA_MODE | SPIF_GC_CPHA | SPIF_GC_CPOL | SPIF_GC_SS | SPIF_GC_HOLD_EN |
		 SPIF_GC_WP_EN | SPIF_GC_DTR_EN);
	writel(val | SPIF_GC_CS_POL, spif->base + SPIF_GC);

	writel(FIELD_PREP(SPIF_CSD_START, 6) | FIELD_PREP(SPIF_CSD_END, 6) |
	       FIELD_PREP(SPIF_CSD_DEASSERT, 5), spif->base + SPIF_CSD);
	writel(FIELD_PREP(SPIF_CFT_RF_EMPTY, 0x10) | FIELD_PREP(SPIF_CFT_RF_FULL, 0x64) |
	       FIELD_PREP(SPIF_CFT_WF_EMPTY, 0x10) | FIELD_PREP(SPIF_CFT_WF_FULL, 0x64),
	       spif->base + SPIF_CFT);

	writel(0, spif->base + SPIF_INT_EN);
	writel(~0, spif->base + SPIF_INT_STA);

	return 0;
}

static int sun300i_spif_probe(struct platform_device *pdev)
{
	struct device *dev = &pdev->dev;
	struct spi_controller *ctlr;
	struct sun300i_spif *spif;
	struct reset_control *rst;
	int irq, ret;

	ctlr = devm_spi_alloc_host(dev, sizeof(*spif));
	if (!ctlr)
		return -ENOMEM;

	spif = spi_controller_get_devdata(ctlr);
	init_completion(&spif->done);

	spif->base = devm_platform_ioremap_resource(pdev, 0);
	if (IS_ERR(spif->base))
		return PTR_ERR(spif->base);

	if (IS_ERR(devm_clk_get_enabled(dev, "bus")))
		return dev_err_probe(dev, -ENODEV, "no bus clock\n");

	spif->mod_clk = devm_clk_get_enabled(dev, "mod");
	if (IS_ERR(spif->mod_clk))
		return dev_err_probe(dev, PTR_ERR(spif->mod_clk), "no mod clock\n");

	rst = devm_reset_control_get_exclusive_deasserted(dev, NULL);
	if (IS_ERR(rst))
		return dev_err_probe(dev, PTR_ERR(rst), "no reset\n");

	ret = dma_set_mask_and_coherent(dev, DMA_BIT_MASK(32));
	if (ret)
		return ret;

	spif->desc = dmam_alloc_coherent(dev, sizeof(*spif->desc), &spif->desc_dma, GFP_KERNEL);
	spif->buf = dmam_alloc_coherent(dev, SPIF_MAX_DATA, &spif->buf_dma, GFP_KERNEL);
	if (!spif->desc || !spif->buf)
		return -ENOMEM;

	ret = sun300i_spif_hw_init(spif);
	if (ret)
		return dev_err_probe(dev, ret, "controller does not come out of reset\n");

	irq = platform_get_irq(pdev, 0);
	if (irq < 0)
		return irq;

	ret = devm_request_irq(dev, irq, sun300i_spif_irq, 0, dev_name(dev), spif);
	if (ret)
		return ret;

	ctlr->mem_ops = &sun300i_spif_mem_ops;
	ctlr->setup = sun300i_spif_setup;
	ctlr->mode_bits = SPI_RX_DUAL | SPI_RX_QUAD | SPI_TX_DUAL | SPI_TX_QUAD;
	ctlr->num_chipselect = 1;
	ctlr->bus_num = -1;
	ctlr->dev.of_node = dev->of_node;

	return devm_spi_register_controller(dev, ctlr);
}

static const struct of_device_id sun300i_spif_of_match[] = {
	{ .compatible = "allwinner,sun300i-v821-spif" },
	{ }
};
MODULE_DEVICE_TABLE(of, sun300i_spif_of_match);

static struct platform_driver sun300i_spif_driver = {
	.probe	= sun300i_spif_probe,
	.driver	= {
		.name		= "sun300i-spif",
		.of_match_table	= sun300i_spif_of_match,
	},
};
module_platform_driver(sun300i_spif_driver);

MODULE_DESCRIPTION("Allwinner V821 SPI flash controller");
MODULE_LICENSE("GPL");
