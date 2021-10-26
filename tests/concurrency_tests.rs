#[cfg(loom)]
mod loom_test {
    use atom_box::{domain::Domain, domain::ReclaimStrategy, AtomBox};
    use loom::sync::Arc;
    use loom::thread;
    use std::convert::From;

    const ITERATIONS: usize = 2;

    #[derive(Debug)]
    struct Value(usize);

    impl From<usize> for Value {
        fn from(value: usize) -> Self {
            Self(value)
        }
    }

    #[test]
    fn concurrency_swap_with_static_ref() {
        let mut builder = loom::model::Builder::new();
        builder.preemption_bound = Some(6);
        builder.check(|| {
            let test_domain: &'static Domain<1> =
                Box::leak(Box::new(Domain::new(ReclaimStrategy::Eager)));

            let atom_box1: &'static _ =
                Box::leak(Box::new(AtomBox::new_with_domain(Value(0), test_domain)));
            let atom_box2: &'static _ =
                Box::leak(Box::new(AtomBox::new_with_domain(Value(0), test_domain)));

            thread::spawn(move || {
                let mut current_value = 0;
                for _ in 1..=ITERATIONS {
                    let new_value = atom_box1.load();
                    assert!(new_value.0 >= current_value, "Value should not decrease");
                    current_value = (*new_value).0;
                }
            });
            thread::spawn(move || {
                for i in 1..=ITERATIONS {
                    let guard1 = atom_box1.swap(Value(i));
                    let value1 = (*guard1).0;
                    let guard2 = atom_box2.swap_from_guard(guard1);
                    assert!(
                        (*guard2).0 <= value1,
                        "Value in first box should be greater than or equal to value in second box"
                    );
                }
            });
        });
    }

    #[test]
    fn concurrency_test_compare_exchange_with_static_ref() {
        let mut builder = loom::model::Builder::new();
        builder.preemption_bound = Some(3);
        builder.check(|| {
            let test_domain: &'static Domain<1> =
                Box::leak(Box::new(Domain::new(ReclaimStrategy::Eager)));

            let atom_box: &'static _ =
                Box::leak(Box::new(AtomBox::new_with_domain(Value(0), test_domain)));

            let handle1 = thread::spawn(move || {
                let mut current_value = atom_box.load();
                let initial_value = (*current_value).0;
                let _ = loop {
                    let new_value = Value((*current_value).0 + 1);
                    match atom_box.compare_exchange(current_value, new_value) {
                        Ok(value) => {
                            break value;
                        }
                        Err(value) => {
                            current_value = value;
                        }
                    }
                };
                let new_value = atom_box.load();
                assert!(
                    (*new_value).0 > initial_value,
                    "Value should have been increased"
                );
            });
            let handle2 = thread::spawn(move || {
                let mut current_value = atom_box.load();
                let initial_value = (*current_value).0;
                let _ = loop {
                    let new_value = Value((*current_value).0 + 1);
                    match atom_box.compare_exchange(current_value, new_value) {
                        Ok(value) => {
                            break value;
                        }
                        Err(value) => {
                            current_value = value;
                        }
                    }
                };
                let new_value = atom_box.load();
                assert!(
                    (*new_value).0 > initial_value,
                    "Value should have been increased"
                );
            });

            match (handle1.join(), handle2.join()) {
                (Ok(_), Ok(_)) => {
                    assert_eq!(atom_box.load().0, 2, "Final value should be 2");
                }
                _ => {
                    panic!("Thread join failed");
                }
            }
        });
    }

    #[test]
    fn concurrency_test_compare_exchange_weak_with_static_ref() {
        let mut builder = loom::model::Builder::new();
        builder.preemption_bound = Some(3);
        builder.check(|| {
            let test_domain: &'static Domain<1> =
                Box::leak(Box::new(Domain::new(ReclaimStrategy::Eager)));

            let atom_box: &'static _ =
                Box::leak(Box::new(AtomBox::new_with_domain(Value(0), test_domain)));

            let handle1 = thread::spawn(move || {
                let mut current_value = atom_box.load();
                let initial_value = (*current_value).0;
                let _ = loop {
                    let new_value = Value((*current_value).0 + 1);
                    match atom_box.compare_exchange_weak(current_value, new_value) {
                        Ok(value) => {
                            break value;
                        }
                        Err(value) => {
                            current_value = value;
                        }
                    }
                };
                let new_value = atom_box.load();
                assert!(
                    (*new_value).0 > initial_value,
                    "Value should have been increased"
                );
            });
            let handle2 = thread::spawn(move || {
                let mut current_value = atom_box.load();
                let initial_value = (*current_value).0;
                let _ = loop {
                    let new_value = Value((*current_value).0 + 1);
                    match atom_box.compare_exchange_weak(current_value, new_value) {
                        Ok(value) => {
                            break value;
                        }
                        Err(value) => {
                            current_value = value;
                        }
                    }
                };
                let new_value = atom_box.load();
                assert!(
                    (*new_value).0 > initial_value,
                    "Value should have been increased"
                );
            });

            match (handle1.join(), handle2.join()) {
                (Ok(_), Ok(_)) => {
                    assert_eq!(atom_box.load().0, 2, "Final value should be 2");
                }
                _ => {
                    panic!("Thread join failed");
                }
            }
        });
    }

    #[test]
    fn concurrency_swap_with_arc() {
        let mut builder = loom::model::Builder::new();
        builder.preemption_bound = Some(3);
        builder.check(|| {
            let test_domain: &'static Domain<1> =
                Box::leak(Box::new(Domain::new(ReclaimStrategy::Eager)));

            let atom_box1 = Arc::new(AtomBox::new_with_domain(Value(0), test_domain));
            let atom_box2 = Arc::new(AtomBox::new_with_domain(Value(0), test_domain));

            let atom_box = atom_box1.clone();
            thread::spawn(move || {
                let mut current_value = 0;
                for _ in 1..=ITERATIONS {
                    let new_value = atom_box.load();
                    assert!(new_value.0 >= current_value, "Value should not decrease");
                    current_value = (*new_value).0;
                }
            });
            let a_box1 = atom_box1.clone();
            let a_box2 = atom_box2.clone();
            thread::spawn(move || {
                for i in 1..=ITERATIONS {
                    let guard1 = a_box1.swap(Value(i));
                    let value1 = (*guard1).0;
                    let guard2 = a_box2.swap_from_guard(guard1);
                    assert!(
                        (*guard2).0 <= value1,
                        "Value in first box should be greater than or equal to value in second box"
                    );
                }
            });
        });
    }

    #[test]
    fn concurrency_test_compare_exchange_with_arc() {
        let mut builder = loom::model::Builder::new();
        builder.preemption_bound = Some(3);
        builder.check(|| {
            let test_domain: &'static Domain<1> =
                Box::leak(Box::new(Domain::new(ReclaimStrategy::Eager)));

            let atom_box = Arc::new(AtomBox::new_with_domain(Value(0), test_domain));

            let atom_box1 = atom_box.clone();
            let handle1 = thread::spawn(move || {
                let mut current_value = atom_box1.load();
                let initial_value = (*current_value).0;
                let _ = loop {
                    let new_value = Value((*current_value).0 + 1);
                    match atom_box1.compare_exchange(current_value, new_value) {
                        Ok(value) => {
                            break value;
                        }
                        Err(value) => {
                            current_value = value;
                        }
                    }
                };
                let new_value = atom_box1.load();
                assert!(
                    (*new_value).0 > initial_value,
                    "Value should have been increased"
                );
            });

            let atom_box2 = atom_box.clone();
            let handle2 = thread::spawn(move || {
                let mut current_value = atom_box2.load();
                let initial_value = (*current_value).0;
                let _ = loop {
                    let new_value = Value((*current_value).0 + 1);
                    match atom_box2.compare_exchange(current_value, new_value) {
                        Ok(value) => {
                            break value;
                        }
                        Err(value) => {
                            current_value = value;
                        }
                    }
                };
                let new_value = atom_box2.load();
                assert!(
                    (*new_value).0 > initial_value,
                    "Value should have been increased"
                );
            });

            match (handle1.join(), handle2.join()) {
                (Ok(_), Ok(_)) => {
                    assert_eq!(atom_box.load().0, 2, "Final value should be 2");
                }
                _ => {
                    panic!("Thread join failed");
                }
            }
        });
    }

    #[test]
    fn concurrency_test_compare_exchange_weak_with_arc() {
        let mut builder = loom::model::Builder::new();
        builder.preemption_bound = Some(3);
        builder.check(|| {
            let test_domain: &'static Domain<1> =
                Box::leak(Box::new(Domain::new(ReclaimStrategy::Eager)));

            let atom_box = Arc::new(AtomBox::new_with_domain(Value(0), test_domain));

            let atom_box1 = atom_box.clone();
            let handle1 = thread::spawn(move || {
                let mut current_value = atom_box1.load();
                let initial_value = (*current_value).0;
                let _ = loop {
                    let new_value = Value((*current_value).0 + 1);
                    match atom_box1.compare_exchange_weak(current_value, new_value) {
                        Ok(value) => {
                            break value;
                        }
                        Err(value) => {
                            current_value = value;
                        }
                    }
                };
                let new_value = atom_box1.load();
                assert!(
                    (*new_value).0 > initial_value,
                    "Value should have been increased"
                );
            });

            let atom_box2 = atom_box.clone();
            let handle2 = thread::spawn(move || {
                let mut current_value = atom_box2.load();
                let initial_value = (*current_value).0;
                let _ = loop {
                    let new_value = Value((*current_value).0 + 1);
                    match atom_box2.compare_exchange_weak(current_value, new_value) {
                        Ok(value) => {
                            break value;
                        }
                        Err(value) => {
                            current_value = value;
                        }
                    }
                };
                let new_value = atom_box2.load();
                assert!(
                    (*new_value).0 > initial_value,
                    "Value should have been increased"
                );
            });

            match (handle1.join(), handle2.join()) {
                (Ok(_), Ok(_)) => {
                    assert_eq!(atom_box.load().0, 2, "Final value should be 2");
                }
                _ => {
                    panic!("Thread join failed");
                }
            }
        });
    }
}
