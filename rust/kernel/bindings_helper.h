/* SPDX-License-Identifier: GPL-2.0 */

#include <linux/cdev.h>
#include <linux/clk.h>
#include <linux/errname.h>
#include <linux/fs.h>
#include <linux/hw_random.h>
#include <linux/module.h>
#include <linux/random.h>
#include <linux/slab.h>
#include <linux/sysctl.h>
#include <linux/uaccess.h>
#include <linux/uio.h>
#include <linux/version.h>
#include <linux/miscdevice.h>
#include <linux/poll.h>
#include <linux/mm.h>
#include <linux/file.h>
#include <uapi/linux/android/binder.h>
#include <linux/platform_device.h>
#include <linux/of_platform.h>
#include <linux/security.h>
#include <asm/io.h>
#include <linux/irq.h>
#include <linux/interrupt.h>
#include <linux/irqdomain.h>
#include <linux/amba/bus.h>
#include <linux/gpio/driver.h>

// `bindgen` gets confused at certain things
const gfp_t BINDINGS_GFP_KERNEL = GFP_KERNEL;
const gfp_t BINDINGS___GFP_ZERO = __GFP_ZERO;
