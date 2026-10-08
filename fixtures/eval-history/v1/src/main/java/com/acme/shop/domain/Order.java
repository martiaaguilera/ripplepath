package com.acme.shop.domain;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

public final class Order {
    private final List<LineItem> items = new ArrayList<>();

    public Order add(Product product, int quantity) {
        items.add(new LineItem(product, quantity));
        return this;
    }

    public List<LineItem> items() {
        return Collections.unmodifiableList(items);
    }

    public Money subtotal() {
        Money total = Money.ZERO;
        for (LineItem item : items) {
            total = total.plus(item.total());
        }
        return total;
    }
}
