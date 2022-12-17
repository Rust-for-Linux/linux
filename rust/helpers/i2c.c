// SPDX-License-Identifier: GPL-2.0

#include <linux/i2c.h>

void *rust_helper_i2c_get_clientdata(const struct i2c_client *client)
{
	return i2c_get_clientdata(client);
}

void rust_helper_i2c_set_clientdata(struct i2c_client *client, void *data)
{
	i2c_set_clientdata(client, data);
}
