// SPDX-License-Identifier: GPL-2.0

#include <linux/i2c.h>
#include <linux/regmap.h>

#if IS_BUILTIN(CONFIG_REGMAP_I2C)
struct regmap *rust_helper_regmap_init_i2c(struct i2c_client *i2c,
					   const struct regmap_config *config)
{
	return regmap_init_i2c(i2c, config);
}
#endif

int rust_helper_regmap_field_write(struct regmap_field *field, unsigned int val)
{
	return regmap_field_write(field, val);
}

int rust_helper_regmap_field_force_write(struct regmap_field *field,
					 unsigned int val)
{
	return regmap_field_force_write(field, val);
}

int rust_helper_regmap_field_update_bits(struct regmap_field *field,
					 unsigned int mask, unsigned int val)
{
	return regmap_field_update_bits(field, mask, val);
}

int rust_helper_regmap_field_set_bits(struct regmap_field *field,
				      unsigned int bits)
{
	return regmap_field_set_bits(field, bits);
}

int rust_helper_regmap_field_clear_bits(struct regmap_field *field,
					unsigned int bits)
{
	return regmap_field_clear_bits(field, bits);
}

int rust_helper_regmap_field_force_update_bits(struct regmap_field *field,
					       unsigned int mask,
						unsigned int val)
{
	return regmap_field_force_update_bits(field, mask, val);
}
