package ferric

// FactType distinguishes ordered from template facts.
type FactType int

const (
	FactOrdered  FactType = iota
	FactTemplate
)

// Fact is an immutable snapshot of a fact in working memory.
type Fact struct {
	ID           uint64
	Type         FactType
	Relation     string         // non-empty for ordered facts
	TemplateName string         // non-empty for template facts
	Fields       []any          // ordered field values
	Slots        map[string]any // template slot values (nil for ordered)
}
