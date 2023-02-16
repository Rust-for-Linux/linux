// SPDX-License-Identifier: GPL-2.0
/*
 * Non-trivial C macros cannot be used in Rust. Similarly, inlined C functions
 * cannot be called either. This file explicitly creates functions ("helpers")
 * that wrap those so that they can be called from Rust.
 *
 * Even though Rust kernel modules should never use directly the bindings, some
 * of these helpers need to be exported because Rust generics and inlined
 * functions may not get their code generated in the crate where they are
 * defined. Other helpers, called from non-inline functions, may not be
 * exported, in principle. However, in general, the Rust compiler does not
 * guarantee codegen will be performed for a non-inline function either.
 * Therefore, this file exports all the helpers. In the future, this may be
 * revisited to reduce the number of exports after the compiler is informed
 * about the places codegen is required.
 *
 * All symbols are exported as GPL-only to guarantee no GPL-only feature is
 * accidentally exposed.
 */

#include <linux/bug.h>
#include <linux/build_bug.h>
#include <linux/device.h>
#include <linux/dma-mapping.h>
#include <linux/err.h>
#include <linux/errname.h>
#include <linux/of.h>
#include <linux/of_device.h>
#include <linux/platform_device.h>
#include <linux/rcupdate.h>
#include <linux/refcount.h>
#include <linux/xarray.h>

__noreturn void rust_helper_BUG(void)
{
	BUG();
}
EXPORT_SYMBOL_GPL(rust_helper_BUG);

refcount_t rust_helper_REFCOUNT_INIT(int n)
{
	return (refcount_t)REFCOUNT_INIT(n);
}
EXPORT_SYMBOL_GPL(rust_helper_REFCOUNT_INIT);

void rust_helper_refcount_inc(refcount_t *r)
{
	refcount_inc(r);
}
EXPORT_SYMBOL_GPL(rust_helper_refcount_inc);

bool rust_helper_refcount_dec_and_test(refcount_t *r)
{
	return refcount_dec_and_test(r);
}
EXPORT_SYMBOL_GPL(rust_helper_refcount_dec_and_test);

__force void *rust_helper_ERR_PTR(long err)
{
	return ERR_PTR(err);
}
EXPORT_SYMBOL_GPL(rust_helper_ERR_PTR);

bool rust_helper_IS_ERR(__force const void *ptr)
{
	return IS_ERR(ptr);
}
EXPORT_SYMBOL_GPL(rust_helper_IS_ERR);

long rust_helper_PTR_ERR(__force const void *ptr)
{
	return PTR_ERR(ptr);
}
EXPORT_SYMBOL_GPL(rust_helper_PTR_ERR);

const char *rust_helper_errname(int err)
{
	return errname(err);
}
EXPORT_SYMBOL_GPL(rust_helper_errname);

void rust_helper_xa_init_flags(struct xarray *xa, gfp_t flags)
{
	xa_init_flags(xa, flags);
}
EXPORT_SYMBOL_GPL(rust_helper_xa_init_flags);

bool rust_helper_xa_empty(struct xarray *xa)
{
	return xa_empty(xa);
}
EXPORT_SYMBOL_GPL(rust_helper_xa_empty);

int rust_helper_xa_alloc(struct xarray *xa, u32 *id, void *entry, struct xa_limit limit, gfp_t gfp)
{
	return xa_alloc(xa, id, entry, limit, gfp);
}
EXPORT_SYMBOL_GPL(rust_helper_xa_alloc);

void rust_helper_xa_lock(struct xarray *xa)
{
	xa_lock(xa);
}
EXPORT_SYMBOL_GPL(rust_helper_xa_lock);

void rust_helper_xa_unlock(struct xarray *xa)
{
	xa_unlock(xa);
}
EXPORT_SYMBOL_GPL(rust_helper_xa_unlock);

int rust_helper_xa_err(void *entry)
{
	return xa_err(entry);
}
EXPORT_SYMBOL_GPL(rust_helper_xa_err);

void *rust_helper_dev_get_drvdata(struct device *dev)
{
	return dev_get_drvdata(dev);
}
EXPORT_SYMBOL_GPL(rust_helper_dev_get_drvdata);

const char *rust_helper_dev_name(const struct device *dev)
{
	return dev_name(dev);
}
EXPORT_SYMBOL_GPL(rust_helper_dev_name);

struct task_struct *rust_helper_get_current(void)
{
	return current;
}
EXPORT_SYMBOL_GPL(rust_helper_get_current);

int rust_helper_signal_pending(struct task_struct *t)
{
	return signal_pending(t);
}
EXPORT_SYMBOL_GPL(rust_helper_signal_pending);

void rust_helper_init_wait(struct wait_queue_entry *wq_entry)
{
	init_wait(wq_entry);
}
EXPORT_SYMBOL_GPL(rust_helper_init_wait);

unsigned long rust_helper_copy_from_user(void *to, const void __user *from, unsigned long n)
{
	return copy_from_user(to, from, n);
}
EXPORT_SYMBOL_GPL(rust_helper_copy_from_user);

unsigned long rust_helper_copy_to_user(void __user *to, const void *from, unsigned long n)
{
	return copy_to_user(to, from, n);
}
EXPORT_SYMBOL_GPL(rust_helper_copy_to_user);

unsigned long rust_helper_clear_user(void __user *to, unsigned long n)
{
	return clear_user(to, n);
}
EXPORT_SYMBOL_GPL(rust_helper_clear_user);

void rust_helper_rcu_read_lock(void)
{
	rcu_read_lock();
}
EXPORT_SYMBOL_GPL(rust_helper_rcu_read_lock);

void rust_helper_rcu_read_unlock(void)
{
	rcu_read_unlock();
}
EXPORT_SYMBOL_GPL(rust_helper_rcu_read_unlock);

void rust_helper_synchronize_rcu(void)
{
	synchronize_rcu();
}
EXPORT_SYMBOL_GPL(rust_helper_synchronize_rcu);

void __iomem *rust_helper_ioremap(resource_size_t offset, unsigned long size)
{
	return ioremap(offset, size);
}
EXPORT_SYMBOL_GPL(rust_helper_ioremap);

void __iomem *rust_helper_ioremap_np(resource_size_t offset, unsigned long size)
{
	return ioremap_np(offset, size);
}
EXPORT_SYMBOL_GPL(rust_helper_ioremap_np);

u8 rust_helper_readb(const volatile void __iomem *addr)
{
	return readb(addr);
}
EXPORT_SYMBOL_GPL(rust_helper_readb);

u16 rust_helper_readw(const volatile void __iomem *addr)
{
	return readw(addr);
}
EXPORT_SYMBOL_GPL(rust_helper_readw);

u32 rust_helper_readl(const volatile void __iomem *addr)
{
	return readl(addr);
}
EXPORT_SYMBOL_GPL(rust_helper_readl);

#ifdef CONFIG_64BIT
u64 rust_helper_readq(const volatile void __iomem *addr)
{
	return readq(addr);
}
EXPORT_SYMBOL_GPL(rust_helper_readq);
#endif

void rust_helper_writeb(u8 value, volatile void __iomem *addr)
{
	writeb(value, addr);
}
EXPORT_SYMBOL_GPL(rust_helper_writeb);

void rust_helper_writew(u16 value, volatile void __iomem *addr)
{
	writew(value, addr);
}
EXPORT_SYMBOL_GPL(rust_helper_writew);

void rust_helper_writel(u32 value, volatile void __iomem *addr)
{
	writel(value, addr);
}
EXPORT_SYMBOL_GPL(rust_helper_writel);

#ifdef CONFIG_64BIT
void rust_helper_writeq(u64 value, volatile void __iomem *addr)
{
	writeq(value, addr);
}
EXPORT_SYMBOL_GPL(rust_helper_writeq);
#endif

u8 rust_helper_readb_relaxed(const volatile void __iomem *addr)
{
	return readb_relaxed(addr);
}
EXPORT_SYMBOL_GPL(rust_helper_readb_relaxed);

u16 rust_helper_readw_relaxed(const volatile void __iomem *addr)
{
	return readw_relaxed(addr);
}
EXPORT_SYMBOL_GPL(rust_helper_readw_relaxed);

u32 rust_helper_readl_relaxed(const volatile void __iomem *addr)
{
	return readl_relaxed(addr);
}
EXPORT_SYMBOL_GPL(rust_helper_readl_relaxed);

#ifdef CONFIG_64BIT
u64 rust_helper_readq_relaxed(const volatile void __iomem *addr)
{
	return readq_relaxed(addr);
}
EXPORT_SYMBOL_GPL(rust_helper_readq_relaxed);
#endif

void rust_helper_writeb_relaxed(u8 value, volatile void __iomem *addr)
{
	writeb_relaxed(value, addr);
}
EXPORT_SYMBOL_GPL(rust_helper_writeb_relaxed);

void rust_helper_writew_relaxed(u16 value, volatile void __iomem *addr)
{
	writew_relaxed(value, addr);
}
EXPORT_SYMBOL_GPL(rust_helper_writew_relaxed);

void rust_helper_writel_relaxed(u32 value, volatile void __iomem *addr)
{
	writel_relaxed(value, addr);
}
EXPORT_SYMBOL_GPL(rust_helper_writel_relaxed);

#ifdef CONFIG_64BIT
void rust_helper_writeq_relaxed(u64 value, volatile void __iomem *addr)
{
	writeq_relaxed(value, addr);
}
EXPORT_SYMBOL_GPL(rust_helper_writeq_relaxed);
#endif

void rust_helper_memcpy_fromio(void *to, const volatile void __iomem *from, long count)
{
	memcpy_fromio(to, from, count);
}
EXPORT_SYMBOL_GPL(rust_helper_memcpy_fromio);

void *
rust_helper_platform_get_drvdata(const struct platform_device *pdev)
{
	return platform_get_drvdata(pdev);
}
EXPORT_SYMBOL_GPL(rust_helper_platform_get_drvdata);

void
rust_helper_platform_set_drvdata(struct platform_device *pdev,
				 void *data)
{
	platform_set_drvdata(pdev, data);
}
EXPORT_SYMBOL_GPL(rust_helper_platform_set_drvdata);

const struct of_device_id *rust_helper_of_match_device(
		const struct of_device_id *matches, const struct device *dev)
{
	return of_match_device(matches, dev);
}
EXPORT_SYMBOL_GPL(rust_helper_of_match_device);

bool rust_helper_of_node_is_root(const struct device_node *np)
{
	return of_node_is_root(np);
}
EXPORT_SYMBOL_GPL(rust_helper_of_node_is_root);

struct device_node *rust_helper_of_parse_phandle(const struct device_node *np,
		const char *phandle_name,
		int index)
{
	return of_parse_phandle(np, phandle_name, index);
}
EXPORT_SYMBOL_GPL(rust_helper_of_parse_phandle);

int rust_helper_dma_set_mask_and_coherent(struct device *dev, u64 mask)
{
	return dma_set_mask_and_coherent(dev, mask);
}
EXPORT_SYMBOL_GPL(rust_helper_dma_set_mask_and_coherent);

resource_size_t rust_helper_resource_size(const struct resource *res)
{
	return resource_size(res);
}
EXPORT_SYMBOL_GPL(rust_helper_resource_size);

void rust_helper_init_completion(struct completion *c)
{
	init_completion(c);
}
EXPORT_SYMBOL_GPL(rust_helper_init_completion);

/*
 * We use `bindgen`'s `--size_t-is-usize` option to bind the C `size_t` type
 * as the Rust `usize` type, so we can use it in contexts where Rust
 * expects a `usize` like slice (array) indices. `usize` is defined to be
 * the same as C's `uintptr_t` type (can hold any pointer) but not
 * necessarily the same as `size_t` (can hold the size of any single
 * object). Most modern platforms use the same concrete integer type for
 * both of them, but in case we find ourselves on a platform where
 * that's not true, fail early instead of risking ABI or
 * integer-overflow issues.
 *
 * If your platform fails this assertion, it means that you are in
 * danger of integer-overflow bugs (even if you attempt to remove
 * `--size_t-is-usize`). It may be easiest to change the kernel ABI on
 * your platform such that `size_t` matches `uintptr_t` (i.e., to increase
 * `size_t`, because `uintptr_t` has to be at least as big as `size_t`).
 */
static_assert(
	sizeof(size_t) == sizeof(uintptr_t) &&
	__alignof__(size_t) == __alignof__(uintptr_t),
	"Rust code expects C `size_t` to match Rust `usize`"
);
