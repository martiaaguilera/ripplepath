package com.acme.shop.domain;

public final class Product {
    private final String sku;
    private final Money price;

    public Product(String sku, Money price) {
        this.sku = sku;
        this.price = price;
    }

    public String sku() {
        return sku;
    }

    public Money price() {
        return price;
    }
}
