package ferric_test

import (
	"context"
	"fmt"
	"log"
	"sort"
	"strings"

	"github.com/plx/ferric-rules/bindings/go"
)

// ExampleEngine demonstrates the basic Engine lifecycle: create, assert
// facts, run, and read captured output.
func ExampleEngine() {
	e, err := ferric.NewEngine(ferric.WithSource(`
		(defrule greet
			(person ?name)
			=>
			(printout t "Hi, " ?name "!" crlf))
	`))
	if err != nil {
		log.Fatal(err)
	}
	defer e.Close()

	if _, err := e.AssertFact("person", ferric.Symbol("Dana")); err != nil {
		log.Fatal(err)
	}

	result, err := e.Run(context.Background())
	if err != nil {
		log.Fatal(err)
	}

	out, _ := e.GetOutput("t")
	fmt.Println(strings.TrimSpace(out))
	fmt.Println("rules fired:", result.RulesFired)
	// Output:
	// Hi, Dana!
	// rules fired: 1
}

// ExampleEngine_AssertTemplate shows how to assert template facts with named
// slots and read them back.
func ExampleEngine_AssertTemplate() {
	e, err := ferric.NewEngine(ferric.WithSource(`
		(deftemplate reading (slot sensor) (slot value))
		(defrule high
			(reading (sensor ?s) (value ?v&:(> ?v 100)))
			=>
			(assert (alert ?s)))
	`))
	if err != nil {
		log.Fatal(err)
	}
	defer e.Close()

	for sensor, value := range map[string]int64{"a": 50, "b": 150} {
		if _, err := e.AssertTemplate("reading", map[string]any{
			"sensor": ferric.Symbol(sensor),
			"value":  value,
		}); err != nil {
			log.Fatal(err)
		}
	}
	if _, err := e.Run(context.Background()); err != nil {
		log.Fatal(err)
	}

	alerts, err := e.FindFacts("alert")
	if err != nil {
		log.Fatal(err)
	}
	for _, f := range alerts {
		fmt.Println("alert:", f.Fields[0])
	}
	// Output:
	// alert: b
}

// ExampleEngine_Reset shows how Reset clears facts between runs while keeping
// compiled rules, giving evaluate-and-discard semantics with one engine.
func ExampleEngine_Reset() {
	e, err := ferric.NewEngine(ferric.WithSource(`
		(defrule echo
			(msg ?text)
			=>
			(printout t ?text crlf))
	`))
	if err != nil {
		log.Fatal(err)
	}
	defer e.Close()

	for _, msg := range []string{"hello", "world"} {
		if err := e.Reset(); err != nil {
			log.Fatal(err)
		}
		if _, err := e.AssertFact("msg", ferric.Symbol(msg)); err != nil {
			log.Fatal(err)
		}
		if _, err := e.Run(context.Background()); err != nil {
			log.Fatal(err)
		}
		out, _ := e.GetOutput("t")
		fmt.Print(out)
		e.ClearOutput("t")
	}
	// Output:
	// hello
	// world
}

// ExampleEngine_Step fires rules one at a time, which is useful for debugging
// or building interactive rule explorers.
func ExampleEngine_Step() {
	e, err := ferric.NewEngine(ferric.WithSource(`
		(defrule step-a (start) => (assert (phase-a)))
		(defrule step-b (phase-a) => (assert (phase-b)))
	`))
	if err != nil {
		log.Fatal(err)
	}
	defer e.Close()

	if _, err := e.AssertFact("start"); err != nil {
		log.Fatal(err)
	}

	steps := 0
	for {
		fired, err := e.Step()
		if err != nil {
			log.Fatal(err)
		}
		if !fired {
			break // agenda empty
		}
		steps++
	}

	facts, err := e.FindFacts("phase-b")
	if err != nil {
		log.Fatal(err)
	}
	fmt.Printf("steps: %d\n", steps)
	fmt.Printf("phase-b asserted: %v\n", len(facts) > 0)
	// Output:
	// steps: 2
	// phase-b asserted: true
}

// ExampleEngine_Rules shows how to inspect rules, templates, and globals.
func ExampleEngine_Rules() {
	e, err := ferric.NewEngine(ferric.WithSource(`
		(deftemplate sensor (slot id) (slot value))
		(defglobal ?*threshold* = 50)
		(defrule check-sensor
			(sensor (id ?id) (value ?v&:(> ?v ?*threshold*)))
			=>
			(printout t "sensor " ?id " above threshold" crlf))
	`))
	if err != nil {
		log.Fatal(err)
	}
	defer e.Close()

	rules := e.Rules()
	fmt.Println("rules:", len(rules))
	for _, r := range rules {
		fmt.Printf("  %s (salience %d)\n", r.Name, r.Salience)
	}

	// Filter to user-defined templates (exclude internal initial-fact).
	var userTemplates []string
	for _, t := range e.Templates() {
		if t != "initial-fact" {
			userTemplates = append(userTemplates, t)
		}
	}
	sort.Strings(userTemplates)
	fmt.Println("templates:", strings.Join(userTemplates, ", "))

	threshold, _ := e.GetGlobal("threshold")
	fmt.Println("threshold:", threshold)
	// Output:
	// rules: 1
	//   check-sensor (salience 0)
	// templates: sensor
	// threshold: 50
}

// ExampleEngine_Serialize snapshots a compiled engine and restores it without
// re-parsing the source.
func ExampleEngine_Serialize() {
	e, err := ferric.NewEngine(ferric.WithSource(`
		(defrule greet (person ?name) => (printout t "Hello, " ?name crlf))
	`))
	if err != nil {
		log.Fatal(err)
	}
	snapshot, err := e.Serialize(ferric.FormatCBOR)
	if err != nil {
		log.Fatal(err)
	}
	_ = e.Close()

	restored, err := ferric.NewEngine(ferric.WithSnapshot(snapshot, ferric.FormatCBOR))
	if err != nil {
		log.Fatal(err)
	}
	defer restored.Close()

	if _, err := restored.AssertFact("person", ferric.Symbol("Ada")); err != nil {
		log.Fatal(err)
	}
	if _, err := restored.Run(context.Background()); err != nil {
		log.Fatal(err)
	}
	out, _ := restored.GetOutput("t")
	fmt.Print(out)
	// Output:
	// Hello, Ada
}
