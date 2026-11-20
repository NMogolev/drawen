/// A compound DSS selector.
///
/// A selector may contain one type name, one id, any number of classes, and
/// zero or more pseudo-classes. For example, `button.primary:hover` is a
/// single compound selector. Combinators between nodes are deliberately not
/// supported yet.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Selector {
    pub(super) is_root: bool,
    pub(super) type_name: Option<String>,
    pub(super) id: Option<String>,
    pub(super) classes: Vec<String>,
    pub(super) pseudo_classes: Vec<PseudoClass>,
}

impl Selector {
    /// Returns whether this selector matches `target`.
    #[must_use]
    pub fn matches(&self, target: &StyleTarget) -> bool {
        if self.is_root != target.is_root {
            return false;
        }
        if self.is_root {
            return true;
        }
        if self
            .type_name
            .as_deref()
            .is_some_and(|type_name| target.type_name.as_deref() != Some(type_name))
        {
            return false;
        }
        if self
            .id
            .as_deref()
            .is_some_and(|id| target.id.as_deref() != Some(id))
        {
            return false;
        }
        self.classes
            .iter()
            .all(|class| target.classes.iter().any(|candidate| candidate == class))
            && self
                .pseudo_classes
                .iter()
                .all(|state| target.states.contains(state))
    }

    /// Returns the selector specificity as `(ids, classes_and_states, types)`.
    #[must_use]
    pub fn specificity(&self) -> (usize, usize, usize) {
        (
            usize::from(self.id.is_some()),
            self.classes.len() + self.pseudo_classes.len() + usize::from(self.is_root),
            usize::from(self.type_name.is_some()),
        )
    }
}

/// Style metadata used to match a [`Selector`].
///
/// Applications will eventually assign this metadata to retained runtime
/// nodes. It is public now so custom widgets and tools can resolve a
/// [`StyleSheet`](super::StyleSheet) using the exact same matching contract.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct StyleTarget {
    pub(super) is_root: bool,
    pub(super) type_name: Option<String>,
    pub(super) id: Option<String>,
    pub(super) classes: Vec<String>,
    pub(super) states: Vec<PseudoClass>,
}

impl StyleTarget {
    /// Creates a target with the given application-defined type name.
    ///
    /// Type names are matched case-insensitively. Use [`Self::empty`] for a
    /// widget that deliberately has no type name and should match only
    /// universal and metadata selectors.
    #[must_use]
    pub fn new(type_name: impl Into<String>) -> Self {
        Self {
            type_name: Some(type_name.into().to_ascii_lowercase()),
            ..Self::default()
        }
    }

    /// Creates a target with no type name, id, classes, or states.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            is_root: false,
            type_name: None,
            id: None,
            classes: Vec::new(),
            states: Vec::new(),
        }
    }

    /// Returns whether this is the `:root` target.
    #[must_use]
    pub const fn is_root(&self) -> bool {
        self.is_root
    }

    /// Returns the target's type name, if any.
    #[must_use]
    pub fn type_name(&self) -> Option<&str> {
        self.type_name.as_deref()
    }

    /// Returns the target's id, if any.
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// Returns the target's classes in insertion order.
    #[must_use]
    pub fn classes(&self) -> &[String] {
        &self.classes
    }

    /// Returns the target's active pseudo-classes in insertion order.
    #[must_use]
    pub fn states(&self) -> &[PseudoClass] {
        &self.states
    }

    /// Replaces the target's type name.
    #[must_use]
    pub fn with_type_name(mut self, type_name: impl Into<String>) -> Self {
        self.type_name = Some(type_name.into().to_ascii_lowercase());
        self
    }

    /// Creates the document root target matched by the `:root` selector.
    #[must_use]
    pub const fn root() -> Self {
        Self {
            is_root: true,
            type_name: None,
            id: None,
            classes: Vec::new(),
            states: Vec::new(),
        }
    }

    /// Adds an id matched by `#id` selectors.
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Adds a class matched by `.class` selectors.
    #[must_use]
    pub fn with_class(mut self, class: impl Into<String>) -> Self {
        let class = class.into();
        if !self.classes.contains(&class) {
            self.classes.push(class);
        }
        self
    }

    /// Adds an active pseudo-class matched by `:state` selectors.
    #[must_use]
    pub fn with_state(mut self, state: PseudoClass) -> Self {
        if !self.states.contains(&state) {
            self.states.push(state);
        }
        self
    }
}

/// Interaction state supported by DSS pseudo-class selectors.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PseudoClass {
    /// The pointer is over the target.
    Hover,
    /// The target is currently active or pressed.
    Pressed,
    /// The target owns keyboard focus.
    Focus,
    /// The target cannot be interacted with.
    Disabled,
}
