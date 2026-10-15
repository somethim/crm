//! Facade opening, index sync and the catalogue use cases (007 steps 1–2).

mod common;

use common::{open, product};
use crm_services::domain::{Money, OwnerInput, OwnerKind, ProductInput, Qty, Sku, SkuId};
use crm_services::{Crm, DataPaths, Fault, Page, ProductPatch, ServiceError};

fn owner_input(kind: OwnerKind, name: &str, description: &str) -> OwnerInput {
    OwnerInput {
        kind,
        name: name.into(),
        description: description.into(),
        nipt: String::new(),
        address: String::new(),
        account_code: String::new(),
    }
}

#[test]
fn open_creates_the_layout_and_the_daily_backup() {
    let dir = tempfile::tempdir().unwrap();
    let crm = Crm::open(
        DataPaths::at(dir.path()),
        &crm_services::DbKey::for_tests(1),
    )
    .unwrap();
    assert!(dir.path().join("data/crm.db").exists());
    assert!(dir.path().join("logs").is_dir());
    let backups = crm.list_backups().unwrap();
    assert_eq!(backups.len(), 1, "{backups:?}");
    assert_eq!(backups[0].name.reason, None, "daily copy");
    assert!(crm.last_backup_at().unwrap().is_some());
    // Not due again today.
    assert!(!crm.daily_backup_if_due().unwrap());
    crm.close().unwrap();
    assert!(matches!(crm.idle_lock_minutes(), Err(ServiceError::Closed)));
    // The rotation keeps the newest regular copy of a day (the exit copy), and
    // reopening takes no second daily copy.
    let crm = Crm::open(
        DataPaths::at(dir.path()),
        &crm_services::DbKey::for_tests(1),
    )
    .unwrap();
    let names: Vec<_> = crm
        .list_backups()
        .unwrap()
        .into_iter()
        .map(|b| b.name.reason)
        .collect();
    assert_eq!(names, vec![Some("exit".to_owned())]);
}

#[test]
fn the_index_sees_a_product_right_after_create() {
    let (_dir, crm) = open();
    assert!(crm.search_products("brizo", 50).unwrap().is_empty());
    let row = product(&crm, "L0009", "BRIZO PJESHKE", 5, 100, 150);
    let found = crm.search_products("brizo pjeshkë", 50).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0], row);

    crm.update_product(
        row.product.id,
        ProductPatch {
            name: Some("COCA COLA".into()),
            ..ProductPatch::default()
        },
    )
    .unwrap();
    assert!(crm.search_products("brizo", 50).unwrap().is_empty());
    assert_eq!(crm.search_products("cola", 50).unwrap().len(), 1);
    crm.delete_product(row.product.id).unwrap();
    assert!(crm.search_products("cola", 50).unwrap().is_empty());
}

/// A commit made outside this facade (another handle on the same file).
fn external_product(crm: &Crm, name: &'static str) {
    let other = crm_storage::Db::open(crm.paths(), &crm_services::DbKey::for_tests(1)).unwrap();
    other
        .write(move |tx| {
            let sku = crm_storage::repo::skus::insert(
                tx,
                &Sku {
                    id: SkuId(0),
                    code: "EXT1".into(),
                    unit: "COPE".into(),
                    buy_price: Money(1),
                    sell_price: Money(1),
                    quantity: Qty(1),
                },
            )?;
            crm_storage::repo::products::insert(tx, sku, name).map(|_| ())
        })
        .unwrap();
    other.checkpoint_and_close().unwrap();
}

#[test]
fn refresh_if_changed_ignores_own_commits_and_notices_external_ones() {
    let (_dir, crm) = open();
    // F-001: the daily backup on open (it records `last_backup_at`) is an own commit.
    assert!(!crm.refresh_if_changed(), "first poll after open");
    product(&crm, "", "X", 1, 1, 1);
    assert!(!crm.refresh_if_changed(), "own use-case write");
    assert_eq!(
        crm.search_products("x", 5).unwrap().len(),
        1,
        "indexed in-process"
    );
    crm.backup_now().unwrap();
    assert!(!crm.refresh_if_changed(), "own backup");

    external_product(&crm, "EXTERNE");
    assert!(crm.search_products("externe", 5).unwrap().is_empty());
    assert!(crm.refresh_if_changed(), "a foreign commit is noticed");
    assert!(!crm.refresh_if_changed());
    assert_eq!(
        crm.search_products("externe", 5).unwrap().len(),
        1,
        "index rebuilt"
    );
}

// Port of CreateProductTest.php:13.
#[test]
fn creates_a_product_with_valid_data() {
    let (_dir, crm) = open();
    let row = crm
        .create_product(ProductInput {
            code: "T0001".into(),
            name: "Test Product".into(),
            unit: "SHISHE".into(),
            buy_price: Money(80_000),
            sell_price: Money(100_000),
            quantity: Qty(50),
        })
        .unwrap();
    assert_eq!(row.product.name, "Test Product");
    assert_eq!(row.sku.code, "T0001");
    assert_eq!(row.sku.unit, "SHISHE");
    assert_eq!(row.sku.buy_price, Money(80_000));
    assert_eq!(row.sku.sell_price, Money(100_000));
    assert_eq!(row.sku.quantity, Qty(50));
    assert_eq!(crm.get_product(row.product.id).unwrap(), row);
}

// Ports of CreateProductTest.php:50 and ProductControllerTest.php:79.
#[test]
fn auto_codes_when_no_code_is_given() {
    let (_dir, crm) = open();
    let a = product(&crm, "", "First", 1, 1, 1);
    let b = product(&crm, "  ", "Second", 1, 1, 1);
    assert_eq!(a.sku.code, "A0001");
    assert_eq!(b.sku.code, "A0002");
    product(&crm, "A0041", "Imported", 1, 1, 1);
    assert_eq!(product(&crm, "", "Third", 1, 1, 1).sku.code, "A0042");
}

#[test]
fn a_flavour_code_joins_the_existing_sku_and_ignores_its_stock_and_prices() {
    let (_dir, crm) = open();
    let base = product(&crm, "L0009", "BRIZO PJESHKE", 30, 100, 150);
    let flavour = product(&crm, "L0009/2", "BRIZO MOLLE", 999, 1, 2);
    assert_eq!(flavour.sku, base.sku);
    let siblings = crm.product_siblings(base.product.id).unwrap();
    assert_eq!(siblings.len(), 1);
    assert_eq!(siblings[0].product.id, flavour.product.id);
    // A SKU whose code contains "L0009" is not a sibling.
    product(&crm, "XL00090", "OTHER", 1, 1, 1);
    assert_eq!(crm.product_siblings(base.product.id).unwrap().len(), 1);
}

// Port of UpdateProductTest.php:56.
#[test]
fn update_changes_only_the_provided_fields() {
    let (_dir, crm) = open();
    let row = product(&crm, "P1", "Original Name", 50, 800, 1_000);
    let updated = crm
        .update_product(
            row.product.id,
            ProductPatch {
                name: Some("Updated Name".into()),
                ..ProductPatch::default()
            },
        )
        .unwrap();
    assert_eq!(updated.product.name, "Updated Name");
    assert_eq!(updated.sku, row.sku);
}

#[test]
fn a_sibling_price_change_shows_on_both_flavours() {
    let (_dir, crm) = open();
    let a = product(&crm, "L0009", "BRIZO PJESHKE", 30, 100, 150);
    let b = product(&crm, "L0009/1", "BRIZO MOLLE", 0, 0, 0);
    crm.update_product(
        b.product.id,
        ProductPatch {
            sell_price: Some(Money(175)),
            quantity: Some(Qty(12)),
            ..ProductPatch::default()
        },
    )
    .unwrap();
    for id in [a.product.id, b.product.id] {
        let row = crm.get_product(id).unwrap();
        assert_eq!(
            (row.sku.sell_price, row.sku.quantity),
            (Money(175), Qty(12))
        );
    }
    // A code change moves both flavours, and the index follows.
    crm.update_product(
        a.product.id,
        ProductPatch {
            code: Some("M0001".into()),
            ..ProductPatch::default()
        },
    )
    .unwrap();
    assert_eq!(crm.get_product(b.product.id).unwrap().sku.code, "M0001");
    assert_eq!(crm.search_products("m0001", 10).unwrap().len(), 2);
}

#[test]
fn a_code_used_by_another_sku_is_refused() {
    let (_dir, crm) = open();
    let a = product(&crm, "A1", "One", 1, 1, 1);
    product(&crm, "B1", "Two", 1, 1, 1);
    let err = crm
        .update_product(
            a.product.id,
            ProductPatch {
                code: Some("B1/3".into()),
                ..ProductPatch::default()
            },
        )
        .unwrap_err();
    let ServiceError::Validation(errors) = &err else {
        panic!("{err:?}");
    };
    assert_eq!(
        errors.messages_for("code"),
        ["Ky kod përdoret nga një artikull tjetër."]
    );
    assert_eq!(
        err.user_message(),
        "Ky kod përdoret nga një artikull tjetër."
    );
    // Its own code is fine.
    crm.update_product(
        a.product.id,
        ProductPatch {
            code: Some("A1".into()),
            ..ProductPatch::default()
        },
    )
    .unwrap();
}

fn field_messages(err: &ServiceError, field: &str) -> Vec<String> {
    let ServiceError::Validation(errors) = err else {
        panic!("expected a validation error, got {err:?}");
    };
    errors
        .messages_for(field)
        .iter()
        .map(ToString::to_string)
        .collect()
}

// F-005: user-reachable uniqueness violations are field errors, not the
// generic constraint message.
#[test]
fn a_duplicate_code_and_name_is_a_name_field_error() {
    let (_dir, crm) = open();
    let first = product(&crm, "D1", "Lëng Molle", 5, 10, 20);
    let input = |code: &str, name: &str| ProductInput {
        code: code.into(),
        name: name.into(),
        unit: "COPE".into(),
        buy_price: Money(10),
        sell_price: Money(20),
        quantity: Qty(1),
    };
    // Same base code (a flavour suffix too) and the same name after trimming.
    for (code, name) in [("D1", "Lëng Molle"), ("D1/2", "  Lëng Molle ")] {
        let err = crm.create_product(input(code, name)).unwrap_err();
        assert_eq!(
            field_messages(&err, "name"),
            ["Ky emër përdoret nga një artikull tjetër me të njëjtin kod."]
        );
    }
    // Nothing was written.
    assert_eq!(crm.product_siblings(first.product.id).unwrap().len(), 0);
    // A different name under the same code, or the same name under another
    // code, is still fine.
    crm.create_product(input("D1", "Lëng Dardhe")).unwrap();
    crm.create_product(input("D2", "Lëng Molle")).unwrap();
}

#[test]
fn renaming_to_a_sibling_name_is_a_name_field_error() {
    let (_dir, crm) = open();
    let a = product(&crm, "R1", "Ujë 0,5", 1, 1, 1);
    let b = product(&crm, "R1", "Ujë 1,5", 1, 1, 1);
    let rename = |id, name: &str| {
        crm.update_product(
            id,
            ProductPatch {
                name: Some(name.into()),
                ..ProductPatch::default()
            },
        )
    };
    let err = rename(b.product.id, " Ujë 0,5").unwrap_err();
    assert_eq!(
        field_messages(&err, "name"),
        ["Ky emër përdoret nga një artikull tjetër me të njëjtin kod."]
    );
    assert_eq!(
        crm.get_product(b.product.id).unwrap().product.name,
        "Ujë 1,5"
    );
    // Keeping its own name, and a free name, still save.
    rename(a.product.id, "Ujë 0,5").unwrap();
    rename(b.product.id, "Ujë 2").unwrap();
    // A product of another SKU may carry the same name.
    let c = product(&crm, "R2", "Tjetër", 1, 1, 1);
    rename(c.product.id, "Ujë 0,5").unwrap();
}

#[test]
fn a_used_account_code_is_an_account_field_error() {
    let (_dir, crm) = open();
    let with_account = |kind, name: &str, account: &str| OwnerInput {
        account_code: account.into(),
        ..owner_input(kind, name, "")
    };
    let first = crm
        .create_owner(with_account(OwnerKind::Client, "Bar Alfa", "411-1"))
        .unwrap();
    let err = crm
        .create_owner(with_account(OwnerKind::Client, "Bar Beta", " 411-1 "))
        .unwrap_err();
    assert_eq!(
        field_messages(&err, "account_code"),
        ["Kjo llogari përdoret nga një klient tjetër."]
    );
    // The same account in the other kind is allowed.
    let supplier = crm
        .create_owner(with_account(OwnerKind::Supplier, "Furnitor", "411-1"))
        .unwrap();
    let err = crm
        .create_owner(with_account(OwnerKind::Supplier, "Furnitor 2", "411-1"))
        .unwrap_err();
    assert_eq!(
        field_messages(&err, "account_code"),
        ["Kjo llogari përdoret nga një furnitor tjetër."]
    );
    // Update: another client's account is refused, its own and blank are fine.
    let second = crm
        .create_owner(with_account(OwnerKind::Client, "Bar Beta", "411-2"))
        .unwrap();
    let err = crm
        .update_owner(
            second.id,
            with_account(OwnerKind::Client, "Bar Beta", "411-1"),
        )
        .unwrap_err();
    assert_eq!(
        field_messages(&err, "account_code"),
        ["Kjo llogari përdoret nga një klient tjetër."]
    );
    assert_eq!(
        crm.get_owner(second.id).unwrap().account_code.as_deref(),
        Some("411-2")
    );
    crm.update_owner(
        first.id,
        with_account(OwnerKind::Client, "Bar Alfa 2", "411-1"),
    )
    .unwrap();
    crm.update_owner(second.id, with_account(OwnerKind::Client, "Bar Beta", ""))
        .unwrap();
    crm.create_owner(with_account(OwnerKind::Client, "Bar Gama", ""))
        .unwrap();
    assert_eq!(
        crm.get_owner(supplier.id).unwrap().account_code.as_deref(),
        Some("411-1")
    );
}

#[test]
fn invalid_product_input_is_a_validation_error() {
    let (_dir, crm) = open();
    let err = crm
        .create_product(ProductInput {
            code: String::new(),
            name: " ".into(),
            unit: "COPE".into(),
            buy_price: Money(-1),
            sell_price: Money(0),
            quantity: Qty(0),
        })
        .unwrap_err();
    assert!(matches!(err, ServiceError::Validation(_)));
    assert!(err.user_message().contains("Emri është i detyrueshëm"));
    assert!(crm.list_products("", Page::ALL).unwrap().is_empty());
}

#[test]
fn deleting_the_last_product_deletes_its_sku() {
    let (_dir, crm) = open();
    let a = product(&crm, "L0009", "BRIZO PJESHKE", 30, 100, 150);
    let b = product(&crm, "L0009/1", "BRIZO MOLLE", 0, 0, 0);
    assert!(!crm.delete_product(a.product.id).unwrap().sku_deleted);
    assert!(crm.delete_product(b.product.id).unwrap().sku_deleted);
    assert!(matches!(
        crm.get_product(b.product.id),
        Err(ServiceError::NotFound)
    ));
    // The code is free again.
    assert_eq!(product(&crm, "L0009", "NEW", 1, 1, 1).sku.quantity, Qty(1));
}

#[test]
fn owners_crud_list_and_search() {
    let (_dir, crm) = open();
    let client = crm
        .create_owner(owner_input(OwnerKind::Client, " Bar Alfa ", "Tiranë"))
        .unwrap();
    assert_eq!(client.name, "Bar Alfa");
    crm.create_owner(owner_input(OwnerKind::Supplier, "Bar Alfa Furnitor", ""))
        .unwrap();
    assert_eq!(
        crm.search_owners(OwnerKind::Client, "alfa", 50).unwrap(),
        std::slice::from_ref(&client)
    );
    assert_eq!(
        crm.list_owners(OwnerKind::Supplier, "", Page::default())
            .unwrap()
            .len(),
        1
    );

    // Port of UpdateSupplierTest.php:31: a blank description clears it.
    let updated = crm
        .update_owner(client.id, owner_input(OwnerKind::Client, "Bar Beta", ""))
        .unwrap();
    assert_eq!(updated.description, None);
    assert_eq!(crm.get_owner(client.id).unwrap(), updated);
    assert!(
        crm.search_owners(OwnerKind::Client, "alfa", 50)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        crm.search_owners(OwnerKind::Client, "beta", 50)
            .unwrap()
            .len(),
        1
    );

    crm.delete_owner(client.id).unwrap();
    assert!(
        crm.search_owners(OwnerKind::Client, "beta", 50)
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        crm.get_owner(client.id),
        Err(ServiceError::NotFound)
    ));
}

#[test]
fn committed_product_writes_are_ok_when_the_index_update_fails() {
    // F-013 variants: create_product / update_product.
    let (_dir, crm) = open();
    crm.inject_fault(Fault::IndexRead);
    let row = product(&crm, "Q1", "QWQWQW", 1, 1, 2);
    assert!(crm.search_products("qwqwqw", 5).unwrap().is_empty());
    assert!(crm.refresh_if_changed());
    assert_eq!(crm.search_products("qwqwqw", 5).unwrap().len(), 1);
    crm.inject_fault(Fault::IndexRead);
    let updated = crm
        .update_product(
            row.product.id,
            ProductPatch {
                name: Some("ZXZXZX".into()),
                ..ProductPatch::default()
            },
        )
        .expect("committed update is Ok");
    assert_eq!(updated.product.name, "ZXZXZX");
    assert!(crm.refresh_if_changed());
    assert_eq!(crm.search_products("zxzxzx", 5).unwrap().len(), 1);
}

#[test]
fn counted_owner_search_lists_50_of_60_matches() {
    let (_dir, crm) = open();
    for i in 0..60 {
        crm.create_owner(owner_input(OwnerKind::Client, &format!("Bar {i:02}"), ""))
            .unwrap();
    }
    crm.create_owner(owner_input(OwnerKind::Supplier, "Bar Furnitor", ""))
        .unwrap();
    let (found, total) = crm
        .search_owners_counted(OwnerKind::Client, "bar", 50)
        .unwrap();
    assert_eq!((found.len(), total), (50, 60));
    let (found, total) = crm
        .search_owners_counted(OwnerKind::Client, "", 50)
        .unwrap();
    assert_eq!((found.len(), total), (50, 60));
    let (found, total) = crm.search_products_counted("", 50).unwrap();
    assert_eq!((found.len(), total), (0, 0));
}
