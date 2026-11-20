/// One compound in a DSS selector complex.
///
/// A compound is the part between combinators, such as `button.primary` or
/// `:not(.danger):hover`. Combinators connect compounds into a right-to-left
/// matching chain.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct CompoundSelector {
    pub(super) type_name: Option<String>,
    pub(super) id: Option<String>,
    pub(super) classes: Vec<String>,
    pub(super) conditions: Vec<Condition>,
}

/// A simple or functional pseudo-class condition inside a compound.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Condition {
    /// A runtime interaction or semantic state.
    State(PseudoClass),
    /// The target is the first child of its parent.
    FirstChild,
    /// The target is the last child of its parent.
    LastChild,
    /// The target is the only child of its parent.
    OnlyChild,
    /// The target's one-based child position matches `An+B`.
    NthChild(NthChild),
    /// None of the argument compounds matches the target.
    Not(Vec<CompoundSelector>),
    /// At least one of the argument compounds matches the target.
    Is(Vec<CompoundSelector>),
}

/// An `An+B` child position expression.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct NthChild {
    a: i32,
    b: i32,
}

impl NthChild {
    pub(super) const fn new(a: i32, b: i32) -> Self {
        Self { a, b }
    }

    /// Returns whether the one-based `position` satisfies `An+B`.
    fn matches(self, position: usize) -> bool {
        let Ok(position) = i32::try_from(position) else {
            return false;
        };
        if self.a == 0 {
            return position == self.b;
        }
        let difference = position - self.b;
        difference.checked_rem(self.a) == Some(0) && difference / self.a >= 0
    }
}

impl CompoundSelector {
    pub(super) fn matches(&self, target: &StyleTarget) -> bool {
        if self.is_empty() {
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
        if !self
            .classes
            .iter()
            .all(|class| target.classes.iter().any(|candidate| candidate == class))
        {
            return false;
        }
        self.conditions
            .iter()
            .all(|condition| condition.matches(target))
    }

    fn is_empty(&self) -> bool {
        self.type_name.is_none()
            && self.id.is_none()
            && self.classes.is_empty()
            && self.conditions.is_empty()
    }

    pub(super) fn specificity(&self) -> (usize, usize, usize) {
        let mut specificity = (
            usize::from(self.id.is_some()),
            self.classes.len(),
            usize::from(self.type_name.is_some()),
        );
        for condition in &self.conditions {
            match condition {
                Condition::State(_)
                | Condition::FirstChild
                | Condition::LastChild
                | Condition::OnlyChild
                | Condition::NthChild(_) => specificity.1 += 1,
                Condition::Not(compounds) | Condition::Is(compounds) => {
                    let inner = compounds
                        .iter()
                        .map(CompoundSelector::specificity)
                        .max()
                        .unwrap_or_default();
                    specificity = (
                        specificity.0.saturating_add(inner.0),
                        specificity.1.saturating_add(inner.1),
                        specificity.2.saturating_add(inner.2),
                    );
                }
            }
        }
        specificity
    }
}

impl Condition {
    fn matches(&self, target: &StyleTarget) -> bool {
        match self {
            Self::State(state) => target.states.contains(state),
            Self::FirstChild => target.position.is_some_and(|position| position.index == 0),
            Self::LastChild => target
                .position
                .is_some_and(|position| position.index + 1 == position.count),
            Self::OnlyChild => target.position.is_some_and(|position| position.count == 1),
            Self::NthChild(nth) => target
                .position
                .is_some_and(|position| nth.matches(position.index + 1)),
            Self::Not(compounds) => !compounds.iter().any(|compound| compound.matches(target)),
            Self::Is(compounds) => compounds.iter().any(|compound| compound.matches(target)),
        }
    }
}

/// A combinator between two compounds of a complex selector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Combinator {
    /// Whitespace: the left compound matches any ancestor.
    Descendant,
    /// `>`: the left compound matches the direct parent.
    Child,
}

/// Position of a node among the children of its parent.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ChildPosition {
    index: usize,
    count: usize,
}

impl ChildPosition {
    /// Creates a zero-based child position with the number of siblings.
    #[must_use]
    pub const fn new(index: usize, count: usize) -> Self {
        Self { index, count }
    }

    /// Returns the zero-based index among siblings.
    #[must_use]
    pub const fn index(self) -> usize {
        self.index
    }

    /// Returns the number of siblings, including this node.
    #[must_use]
    pub const fn count(self) -> usize {
        self.count
    }
}

/// A parsed DSS selector.
///
/// A selector is a chain of compounds joined by descendant or child
/// combinators. The rightmost compound matches the styled node; the remaining
/// compounds match its ancestors. For example, `toolbar > button.primary`
/// matches a primary button whose direct parent is a toolbar.
///
/// The stylesheet parser normalizes selectors, so custom tooling should treat
/// the type as an opaque matcher rather than reconstructing source text.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Selector {
    pub(super) compounds: Vec<CompoundSelector>,
    pub(super) combinators: Vec<Combinator>,
    pub(super) is_root: bool,
    pub(super) specificity: (usize, usize, usize),
}

impl Selector {
    /// Returns whether this selector matches `target`.
    #[must_use]
    pub fn matches(&self, target: &StyleTarget) -> bool {
        if self.is_root {
            return target.is_root;
        }
        if target.is_root || self.compounds.is_empty() {
            return false;
        }
        let Some((last, rest)) = self.compounds.split_last() else {
            return false;
        };
        if !last.matches(target) {
            return false;
        }
        let Some(combinators) = self.combinators.get(..self.compounds.len() - 1) else {
            return false;
        };
        let mut ancestors = target.ancestors().iter();
        for (index, combinator) in combinators.iter().enumerate().rev() {
            let compound = &rest[index];
            match combinator {
                Combinator::Child => {
                    let Some(ancestor) = ancestors.next() else {
                        return false;
                    };
                    if !compound.matches(ancestor) {
                        return false;
                    }
                }
                Combinator::Descendant => {
                    if !ancestors
                        .by_ref()
                        .any(|ancestor| compound.matches(ancestor))
                    {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Returns the selector specificity as `(ids, classes_and_states, types)`.
    #[must_use]
    pub const fn specificity(&self) -> (usize, usize, usize) {
        self.specificity
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
    pub(super) position: Option<ChildPosition>,
    /// Nearest ancestor first.
    pub(super) ancestors: Vec<StyleTarget>,
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
            position: None,
            ancestors: Vec::new(),
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

    /// Returns the target's position among its siblings, if the target belongs
    /// to a retained tree.
    #[must_use]
    pub const fn child_position(&self) -> Option<ChildPosition> {
        self.position
    }

    /// Returns the nearest-first ancestor metadata usable by combinators.
    ///
    /// Ancestor entries are shallow: they describe the ancestor itself and do
    /// not repeat their own ancestry.
    #[must_use]
    pub fn ancestors(&self) -> &[Self] {
        &self.ancestors
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
            position: None,
            ancestors: Vec::new(),
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

    /// Records the target's zero-based position and sibling count.
    #[must_use]
    pub const fn with_child_position(mut self, index: usize, count: usize) -> Self {
        self.position = Some(ChildPosition::new(index, count));
        self
    }

    /// Prepends `parent` to this target's ancestor chain.
    ///
    /// Retained runtimes call this while walking the tree so descendant and
    /// child selectors can match. The parent's own ancestry is copied, and the
    /// stored record is shallow, so chains grow linearly with tree depth.
    #[must_use]
    pub fn with_parent(mut self, parent: &Self) -> Self {
        let mut record = parent.clone();
        record.ancestors = Vec::new();
        self.ancestors = Vec::with_capacity(parent.ancestors.len() + 1);
        self.ancestors.push(record);
        self.ancestors.extend(parent.ancestors.iter().cloned());
        self
    }
}

/// Interaction and semantic state supported by DSS pseudo-class selectors.
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
    /// The target is a checked toggle or mixed checkbox.
    Checked,
    /// The target is selected inside a list, tab bar, or similar container.
    Selected,
}

impl PseudoClass {
    /// Returns the canonical `:` name used in selectors and diagnostics.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Hover => "hover",
            Self::Pressed => "pressed",
            Self::Focus => "focus",
            Self::Disabled => "disabled",
            Self::Checked => "checked",
            Self::Selected => "selected",
        }
    }
}
