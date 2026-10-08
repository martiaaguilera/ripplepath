package com.acme.shop.application;

import static org.junit.jupiter.api.Assertions.assertEquals;

import com.acme.shop.domain.Money;
import com.acme.shop.domain.Order;
import com.acme.shop.domain.Product;
import com.acme.shop.inventory.Inventory;
import com.acme.shop.pricing.BulkDiscount;
import com.acme.shop.pricing.PercentageDiscount;
import com.acme.shop.pricing.TaxCalculator;
import com.acme.shop.shipping.ShippingCalculator;
import org.junit.jupiter.api.Test;

class CheckoutServiceTest {
    private static Inventory stocked(String sku, int quantity) {
        Inventory inventory = new Inventory();
        inventory.restock(sku, quantity);
        return inventory;
    }

    private static CheckoutService service(Inventory inventory) {
        return new CheckoutService(
                inventory, new PercentageDiscount(10), TaxCalculator.fromResource(), new ShippingCalculator());
    }

    @Test
    void totalsDiscountTaxAndShipping() {
        Order order = new Order().add(new Product("book", Money.ofCents(1250)), 2);
        Receipt receipt = service(stocked("book", 10)).checkout(order, "ES");
        assertEquals(Money.ofCents(2500), receipt.subtotal());
        assertEquals(Money.ofCents(250), receipt.discount());
        assertEquals(Money.ofCents(473), receipt.tax());
        assertEquals(Money.ofCents(495), receipt.shipping());
        assertEquals(Money.ofCents(3218), receipt.total());
    }

    @Test
    void freeShippingFromThreshold() {
        Order order = new Order().add(new Product("chair", Money.ofCents(5000)), 1);
        Receipt receipt = service(stocked("chair", 1)).checkout(order, "DE");
        assertEquals(Money.ZERO, receipt.shipping());
    }

    @Test
    void bulkOrdersGetTheBulkDiscount() {
        CheckoutService service = new CheckoutService(
                stocked("pen", 10), new BulkDiscount(15), TaxCalculator.fromResource(), new ShippingCalculator());
        Order order = new Order().add(new Product("pen", Money.ofCents(200)), 10);
        assertEquals(Money.ofCents(300), service.checkout(order, "ES").discount());
    }

    @Test
    void reservesStockForEveryItem() {
        Inventory inventory = stocked("book", 5);
        Order order = new Order().add(new Product("book", Money.ofCents(1250)), 5);
        service(inventory).checkout(order, "FR");
        assertEquals(0, inventory.available("book"));
    }
}
