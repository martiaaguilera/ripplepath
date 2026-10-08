package com.acme.shop.application;

import com.acme.shop.domain.Money;

public final class Receipt {
    private final Money subtotal;
    private final Money discount;
    private final Money tax;
    private final Money shipping;
    private final Money total;

    public Receipt(Money subtotal, Money discount, Money tax, Money shipping, Money total) {
        this.subtotal = subtotal;
        this.discount = discount;
        this.tax = tax;
        this.shipping = shipping;
        this.total = total;
    }

    public Money subtotal() {
        return subtotal;
    }

    public Money discount() {
        return discount;
    }

    public Money tax() {
        return tax;
    }

    public Money shipping() {
        return shipping;
    }

    public Money total() {
        return total;
    }
}
