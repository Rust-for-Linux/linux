// SPDX-License-Identifier: GPL-2.0

#include <linux/of_device.h>

const struct of_device_id *rust_helper_of_match_device(
		const struct of_device_id *matches, const struct device *dev)
{
	return of_match_device(matches, dev);
}
