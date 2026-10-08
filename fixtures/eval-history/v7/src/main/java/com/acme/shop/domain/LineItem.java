package com.acme.shop.domain;

public final class LineItem {
    private final Product product;
    private final int quantity;

    public LineItem(Product product, int quantity) {
        if (quantity <= 0) {
            throw new IllegalArgumentException("quantity must be positive: " + quantity);
        }
        this.product = product;
        this.quantity = quantity;
    }

    public Product product() {
        return product;
    }

    public int quantity() {
        return quantity;
    }

    public Money total() {
        return product.price().times(quantity);
    }
}
