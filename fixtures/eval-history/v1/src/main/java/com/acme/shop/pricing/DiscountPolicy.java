package com.acme.shop.pricing;

import com.acme.shop.domain.Money;
import com.acme.shop.domain.Order;

public interface DiscountPolicy {
    Money discountFor(Order order);
}
