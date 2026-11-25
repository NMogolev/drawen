use std::{
    cell::{Cell, RefCell},
    fmt,
    rc::{Rc, Weak},
};

use crate::{Signal, Subscription};

/// Shared invalidation flags a runtime drains after watched properties change.
///
/// A [`Property`] holds a weak handle to the flag installed by
/// [`Self::watch`]; a runtime owns the strong handle and converts raised flags
/// into its normal paint or layout invalidation. The flag never calls back
/// into widgets or the runtime: recording damage is two cell writes, so it is
/// safe inside signal emission.
#[derive(Debug, Default)]
pub struct PropertyDamage {
    paint: Cell<bool>,
    layout: Cell<bool>,
}

impl PropertyDamage {
    /// Creates a flag with neither paint nor layout damage raised.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that retained paint output must be rebuilt.
    pub fn mark_paint(&self) {
        self.paint.set(true);
    }

    /// Records that retained layout and paint output must be rebuilt.
    pub fn mark_layout(&self) {
        self.layout.set(true);
    }

    /// Clears and returns whether paint damage was recorded.
    pub fn take_paint(&self) -> bool {
        self.paint.replace(false)
    }

    /// Clears and returns whether layout damage was recorded.
    pub fn take_layout(&self) -> bool {
        self.layout.replace(false)
    }
}

struct Watcher {
    damage: Weak<PropertyDamage>,
    layout: bool,
}

struct Inner<T> {
    value: RefCell<T>,
    changed: Signal<T>,
    watchers: RefCell<Vec<Watcher>>,
}

/// A value-carrying property with a synchronous change signal.
///
/// A property is the addressable form of widget or application state: clones
/// share one value, [`Self::set`] stores a new value and emits
/// [`Self::changed`] only when the value actually differs, and equal writes
/// are silent. That deduplication is what keeps chained or cyclic bindings
/// convergent instead of looping.
///
/// Properties are UI-thread-local like [`Signal`]. A runtime can
/// [`watch`](Self::watch) a property so every change raises a
/// [`PropertyDamage`] flag the runtime drains into paint or layout
/// invalidation; `UiRuntime::watch` and `UiRuntime::bind` install that flag
/// for widget properties.
#[must_use = "a property handle keeps its shared value alive"]
pub struct Property<T> {
    inner: Rc<Inner<T>>,
}

impl<T: Clone + PartialEq + 'static> Property<T> {
    /// Creates a property holding `value`.
    pub fn new(value: T) -> Self {
        Self {
            inner: Rc::new(Inner {
                value: RefCell::new(value),
                changed: Signal::new(),
                watchers: RefCell::new(Vec::new()),
            }),
        }
    }

    /// Returns a clone of the current value.
    #[must_use]
    pub fn get(&self) -> T {
        self.inner.value.borrow().clone()
    }

    /// Stores `value` and returns whether it differed from the current one.
    ///
    /// A changed value notifies watched damage flags, then emits
    /// [`Self::changed`] with the new value. An equal value changes nothing
    /// and emits nothing, so bindings built on properties cannot ping-pong.
    /// A set from inside a running listener is stored, but that listener is
    /// not re-entered, matching [`Signal::emit`].
    pub fn set(&self, value: T) -> bool {
        {
            let mut current = self.inner.value.borrow_mut();
            if *current == value {
                return false;
            }
            *current = value;
        }
        self.notify_watchers();
        let value = self.inner.value.borrow().clone();
        self.inner.changed.emit(&value);
        true
    }

    /// Returns the signal emitted with each changed value.
    #[must_use]
    pub fn changed(&self) -> Signal<T> {
        self.inner.changed.clone()
    }

    /// Adds a listener that receives each changed value.
    ///
    /// Shorthand for `self.changed().subscribe(listener)`; keep the returned
    /// [`Subscription`] alive for as long as notifications are wanted.
    pub fn subscribe(&self, listener: impl FnMut(&T) + 'static) -> Subscription {
        self.inner.changed.subscribe(listener)
    }

    /// Raises `damage` whenever the stored value changes.
    ///
    /// `layout` selects layout damage (which implies paint damage) instead of
    /// paint-only damage. Watching the same flag twice keeps one watcher and
    /// upgrades it to layout damage when either request asks for it. The
    /// property keeps only a weak handle, so a dropped runtime leaves the
    /// property usable. `UiRuntime::watch` and `UiRuntime::bind` call this;
    /// applications rarely need it directly.
    pub fn watch(&self, damage: &Rc<PropertyDamage>, layout: bool) {
        let mut watchers = self.inner.watchers.borrow_mut();
        let incoming = Rc::downgrade(damage);
        if let Some(existing) = watchers
            .iter_mut()
            .find(|watcher| watcher.damage.ptr_eq(&incoming))
        {
            existing.layout |= layout;
            return;
        }
        watchers.push(Watcher {
            damage: incoming,
            layout,
        });
    }

    fn notify_watchers(&self) {
        self.inner.watchers.borrow_mut().retain(|watcher| {
            let Some(damage) = watcher.damage.upgrade() else {
                return false;
            };
            if watcher.layout {
                damage.mark_layout();
            } else {
                damage.mark_paint();
            }
            true
        });
    }
}

impl<T> Clone for Property<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T> fmt::Debug for Property<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Property")
            .field("watcher_count", &self.inner.watchers.borrow().len())
            .finish_non_exhaustive()
    }
}

/// A drop-based property binding created by `UiRuntime::bind`.
///
/// The binding owns the subscription that copies each source change into the
/// target property. Dropping it disconnects the copy immediately, exactly
/// like dropping a [`Subscription`]; the target keeps its last value and any
/// damage watch installed for it.
#[must_use = "dropping a binding disconnects it immediately"]
pub struct Binding {
    subscription: Subscription,
}

impl Binding {
    /// Wraps the subscription that drives a binding.
    pub fn new(subscription: Subscription) -> Self {
        Self { subscription }
    }

    /// Disconnects the binding now. Calling this more than once is harmless.
    pub fn unsubscribe(&mut self) {
        self.subscription.unsubscribe();
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::{Property, PropertyDamage};

    #[test]
    fn set_deduplicates_equal_values() {
        let property = Property::new(1_u32);
        let observed = Rc::new(RefCell::new(Vec::new()));
        let _subscription = property.subscribe({
            let observed = Rc::clone(&observed);
            move |value| observed.borrow_mut().push(*value)
        });

        assert!(property.set(2));
        assert!(!property.set(2));
        assert!(property.set(3));

        assert_eq!(property.get(), 3);
        assert_eq!(*observed.borrow(), vec![2, 3]);
    }

    #[test]
    fn clones_share_one_value() {
        let property = Property::new(String::from("idle"));
        let handle = property.clone();
        handle.set(String::from("busy"));
        assert_eq!(property.get(), "busy");
    }

    #[test]
    fn changed_emits_the_stored_value() {
        let property = Property::new(false);
        let observed = Rc::new(RefCell::new(Vec::new()));
        let _subscription = property.changed().subscribe({
            let observed = Rc::clone(&observed);
            move |value| observed.borrow_mut().push(*value)
        });

        property.set(true);
        property.set(true);
        property.set(false);

        assert_eq!(*observed.borrow(), vec![true, false]);
    }

    #[test]
    fn reentrant_set_during_emission_stores_without_recursing() {
        let property = Property::new(0_u32);
        let observed = Rc::new(RefCell::new(Vec::new()));
        let _subscription = property.subscribe({
            let property = property.clone();
            let observed = Rc::clone(&observed);
            move |value| {
                observed.borrow_mut().push(*value);
                if *value == 1 {
                    property.set(2);
                }
            }
        });

        property.set(1);

        // The running listener is not re-entered; the nested write is stored
        // and the next emission delivers normally.
        assert_eq!(property.get(), 2);
        assert_eq!(*observed.borrow(), vec![1]);
        property.set(3);
        assert_eq!(*observed.borrow(), vec![1, 3]);
    }

    #[test]
    fn watch_records_paint_or_layout_damage() {
        let property = Property::new(0_u32);
        let damage = Rc::new(PropertyDamage::new());
        property.watch(&damage, false);

        property.set(1);
        assert!(damage.take_paint());
        assert!(!damage.take_layout());
        assert!(!damage.take_paint());

        property.set(1);
        assert!(!damage.take_paint());
    }

    #[test]
    fn watching_one_flag_twice_upgrades_to_layout_damage() {
        let property = Property::new(0_u32);
        let damage = Rc::new(PropertyDamage::new());
        property.watch(&damage, false);
        property.watch(&damage, true);

        property.set(1);

        assert!(damage.take_layout());
        assert!(!damage.take_paint());
    }

    #[test]
    fn a_dropped_damage_flag_leaves_the_property_usable() {
        let property = Property::new(0_u32);
        {
            let damage = Rc::new(PropertyDamage::new());
            property.watch(&damage, false);
        }
        assert!(property.set(1));
        assert_eq!(property.get(), 1);
    }
}
