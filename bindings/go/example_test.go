package ferric_test

import (
	"context"
	"fmt"
	"log"
	"sort"
	"strings"

	"github.com/prb/ferric-rules/bindings/go"
)

// ---------------------------------------------------------------------------
// Pattern 1: Stateless — Manager.Evaluate
// ---------------------------------------------------------------------------
//
// The Coordinator+Manager pattern is the primary way to evaluate rules in a
// request/response style. Each Evaluate call resets the engine, asserts the
// supplied facts, runs to completion, and returns the resulting facts and
// captured output. No state carries over between calls.
//
// Thread affinity is handled automatically: the Coordinator manages a pool of
// OS-locked worker goroutines and dispatches each request to one of them.
// Callers never need to worry about runtime.LockOSThread.

// ExampleManager_Evaluate demonstrates stateless one-shot evaluation using
// the wire-type API. Facts are supplied as WireFactInput values and results
// are returned as WireFact values — both are JSON-serializable, making this
// pattern ideal for RPC boundaries and Temporal activities.
func ExampleManager_Evaluate() {
	mgr, err := ferric.NewManager(ferric.WithSource(`
		(defrule greet
			(person ?name)
			=>
			(printout t "Hello, " ?name "!" crlf))
	`))
	if err != nil {
		log.Fatal(err)
	}
	defer mgr.Close()

	result, err := mgr.Evaluate(context.Background(), &ferric.EvaluateRequest{
		Facts: []ferric.WireFactInput{
			ferric.OrderedFact("person", ferric.SymbolValue("Alice")),
		},
	})
	if err != nil {
		log.Fatal(err)
	}

	fmt.Println("rules fired:", result.RulesFired)
	fmt.Println("output:", strings.TrimSpace(result.Output["stdout"]))
	// Output:
	// rules fired: 1
	// output: Hello, Alice!
}

// ExampleManager_Evaluate_stateless shows that successive Evaluate calls are
// completely independent — facts from one call do not carry into the next.
func ExampleManager_Evaluate_stateless() {
	mgr, err := ferric.NewManager(ferric.WithSource(`
		(defrule tag
			(color ?c)
			=>
			(printout t ?c " "))
	`))
	if err != nil {
		log.Fatal(err)
	}
	defer mgr.Close()

	// First call: assert "red".
	r1, err := mgr.Evaluate(context.Background(), &ferric.EvaluateRequest{
		Facts: []ferric.WireFactInput{
			ferric.OrderedFact("color", ferric.SymbolValue("red")),
		},
	})
	if err != nil {
		log.Fatal(err)
	}

	// Second call: assert "blue". "red" does NOT carry over.
	r2, err := mgr.Evaluate(context.Background(), &ferric.EvaluateRequest{
		Facts: []ferric.WireFactInput{
			ferric.OrderedFact("color", ferric.SymbolValue("blue")),
		},
	})
	if err != nil {
		log.Fatal(err)
	}

	fmt.Println("call 1:", strings.TrimSpace(r1.Output["stdout"]))
	fmt.Println("call 2:", strings.TrimSpace(r2.Output["stdout"]))
	// Output:
	// call 1: red
	// call 2: blue
}

// ExampleManager_EvaluateNative demonstrates the Go-convenience API that
// accepts native Go types (int, string, ferric.Symbol, etc.) instead of
// wire types. This is more ergonomic for pure-Go callers that don't need
// cross-language serialization.
func ExampleManager_EvaluateNative() {
	mgr, err := ferric.NewManager(ferric.WithSource(`
		(deftemplate sensor (slot id) (slot temp))
		(defrule hot-sensor
			(sensor (id ?id) (temp ?t&:(> ?t 100)))
			=>
			(printout t "ALERT: sensor " ?id " reads " ?t crlf))
	`))
	if err != nil {
		log.Fatal(err)
	}
	defer mgr.Close()

	result, err := mgr.EvaluateNative(context.Background(), &ferric.EvaluateNativeRequest{
		Facts: []ferric.NativeFactInput{
			{TemplateName: "sensor", Slots: map[string]any{"id": int64(1), "temp": int64(42)}},
			{TemplateName: "sensor", Slots: map[string]any{"id": int64(2), "temp": int64(105)}},
		},
	})
	if err != nil {
		log.Fatal(err)
	}

	fmt.Println("rules fired:", result.RulesFired)
	fmt.Println(strings.TrimSpace(result.Output["stdout"]))
	// Output:
	// rules fired: 1
	// ALERT: sensor 2 reads 105
}

// ExampleManager_Evaluate_templateFacts shows how to assert template facts
// (named slots) via the wire-type API.
func ExampleManager_Evaluate_templateFacts() {
	mgr, err := ferric.NewManager(ferric.WithSource(`
		(deftemplate order (slot item) (slot qty))
		(defrule summarize
			(order (item ?i) (qty ?q))
			=>
			(printout t ?i ": " ?q crlf))
	`))
	if err != nil {
		log.Fatal(err)
	}
	defer mgr.Close()

	result, err := mgr.Evaluate(context.Background(), &ferric.EvaluateRequest{
		Facts: []ferric.WireFactInput{
			ferric.TemplateFact("order", map[string]ferric.WireValue{
				"item": ferric.SymbolValue("widget"),
				"qty":  ferric.IntValue(5),
			}),
		},
	})
	if err != nil {
		log.Fatal(err)
	}

	fmt.Println(strings.TrimSpace(result.Output["stdout"]))
	// Output:
	// widget: 5
}

// ---------------------------------------------------------------------------
// Pattern 2: Stateful escape hatch — Manager.Do
// ---------------------------------------------------------------------------
//
// Manager.Do dispatches a closure to an engine on a thread-locked worker.
// Inside the closure you have direct access to the Engine and can perform
// arbitrary sequences of operations. The engine persists across Do calls on
// the same worker thread, so rules are compiled only once.
//
// IMPORTANT: The *Engine passed to the closure must not be retained or used
// after the closure returns — doing so violates thread affinity and will
// panic or produce undefined behavior.

// ExampleManager_Do demonstrates the Do escape hatch for multi-step
// engine interaction that goes beyond simple evaluate-and-discard.
func ExampleManager_Do() {
	mgr, err := ferric.NewManager(ferric.WithSource(`
		(deftemplate account (slot name) (slot balance))

		(defrule low-balance
			(account (name ?n) (balance ?b&:(< ?b 100)))
			=>
			(printout t "LOW: " ?n " $" ?b crlf))
	`))
	if err != nil {
		log.Fatal(err)
	}
	defer mgr.Close()

	ctx := context.Background()
	err = mgr.Do(ctx, func(e *ferric.Engine) error {
		// Reset clears facts but keeps compiled rules.
		if err := e.Reset(); err != nil {
			return err
		}

		// Assert several facts programmatically.
		accounts := []struct {
			name    string
			balance int64
		}{
			{"Alice", 250},
			{"Bob", 50},
			{"Carol", 75},
		}
		for _, a := range accounts {
			if _, err := e.AssertTemplate("account", map[string]any{
				"name":    ferric.Symbol(a.name),
				"balance": a.balance,
			}); err != nil {
				return err
			}
		}

		// Run the engine.
		result, err := e.Run(ctx)
		if err != nil {
			return err
		}

		// Inspect output.
		if out, ok := e.GetOutput("t"); ok {
			lines := strings.Split(strings.TrimSpace(out), "\n")
			sort.Strings(lines)
			for _, line := range lines {
				fmt.Println(line)
			}
		}
		fmt.Println("rules fired:", result.RulesFired)
		return nil
	})
	if err != nil {
		log.Fatal(err)
	}
	// Output:
	// LOW: Bob $50
	// LOW: Carol $75
	// rules fired: 2
}

// ExampleManager_Do_stepExecution uses Do to step through rule firings
// one at a time, which is useful for debugging or building interactive
// rule explorers.
func ExampleManager_Do_stepExecution() {
	mgr, err := ferric.NewManager(ferric.WithSource(`
		(defrule step-a (start) => (assert (phase-a)))
		(defrule step-b (phase-a) => (assert (phase-b)))
	`))
	if err != nil {
		log.Fatal(err)
	}
	defer mgr.Close()

	err = mgr.Do(context.Background(), func(e *ferric.Engine) error {
		if err := e.Reset(); err != nil {
			return err
		}
		if _, err := e.AssertFact("start"); err != nil {
			return err
		}

		// Step through one rule at a time.
		steps := 0
		for {
			fired, err := e.Step()
			if err != nil {
				return err
			}
			if fired == nil {
				break // agenda empty
			}
			steps++
		}

		// Verify chained facts were produced.
		facts, err := e.FindFacts("phase-b")
		if err != nil {
			return err
		}
		fmt.Printf("steps: %d\n", steps)
		fmt.Printf("phase-b asserted: %v\n", len(facts) > 0)
		return nil
	})
	if err != nil {
		log.Fatal(err)
	}
	// Output:
	// steps: 2
	// phase-b asserted: true
}

// ---------------------------------------------------------------------------
// Pattern 3: Stateful — PinnedEngine
// ---------------------------------------------------------------------------
//
// PinnedEngine wraps a single engine on a dedicated OS-locked goroutine.
// All operations are serialized through that goroutine in FIFO order, so
// PinnedEngine is safe for concurrent use from any number of goroutines
// without callers needing to manage thread affinity themselves.
//
// State persists across calls: facts, rules, and globals survive until
// explicitly cleared. This makes PinnedEngine ideal for long-lived
// workflows, interactive REPL-style use, and applications that accumulate
// knowledge over time.
//
// Thread-affinity note: The underlying ferric engine is bound to the OS
// thread that created it (via runtime.LockOSThread). PinnedEngine handles
// this transparently — you call methods from any goroutine and they are
// dispatched to the correct thread automatically.

// ExamplePinnedEngine demonstrates the basic PinnedEngine lifecycle:
// create, load rules, assert facts, run, inspect results.
func ExamplePinnedEngine() {
	p, err := ferric.NewPinnedEngine(ferric.WithSource(`
		(defrule greet
			(person ?name)
			=>
			(printout t "Hi, " ?name "!" crlf))
	`))
	if err != nil {
		log.Fatal(err)
	}
	defer p.Close()

	if _, err := p.AssertFact("person", ferric.Symbol("Dana")); err != nil {
		log.Fatal(err)
	}

	result, err := p.Run(context.Background())
	if err != nil {
		log.Fatal(err)
	}

	out, _ := p.GetOutput("t")
	fmt.Println(strings.TrimSpace(out))
	fmt.Println("rules fired:", result.RulesFired)
	// Output:
	// Hi, Dana!
	// rules fired: 1
}

// ExamplePinnedEngine_statePersistence shows that facts persist across
// multiple Run calls — new facts accumulate alongside previously asserted
// ones unless you explicitly call Reset.
func ExamplePinnedEngine_statePersistence() {
	p, err := ferric.NewPinnedEngine(ferric.WithSource(`
		(defrule count
			(item ?x)
			=>
			(printout t "seen: " ?x crlf))
	`))
	if err != nil {
		log.Fatal(err)
	}
	defer p.Close()

	// First run: assert and fire.
	if _, err := p.AssertFact("item", ferric.Symbol("apple")); err != nil {
		log.Fatal(err)
	}
	r1, err := p.Run(context.Background())
	if err != nil {
		log.Fatal(err)
	}

	// Second run: assert another fact. "apple" is still in working memory,
	// but its rule already fired so only the new fact triggers.
	if _, err := p.AssertFact("item", ferric.Symbol("banana")); err != nil {
		log.Fatal(err)
	}
	r2, err := p.Run(context.Background())
	if err != nil {
		log.Fatal(err)
	}

	out, _ := p.GetOutput("t")
	fmt.Println(strings.TrimSpace(out))
	fmt.Printf("run 1 fired: %d, run 2 fired: %d\n", r1.RulesFired, r2.RulesFired)
	// Output:
	// seen: apple
	// seen: banana
	// run 1 fired: 1, run 2 fired: 1
}

// ExamplePinnedEngine_resetBetweenRuns shows how to use Reset to clear
// facts between runs while keeping compiled rules. This gives you
// evaluate-and-discard semantics with a long-lived engine.
func ExamplePinnedEngine_resetBetweenRuns() {
	p, err := ferric.NewPinnedEngine(ferric.WithSource(`
		(defrule echo
			(msg ?text)
			=>
			(printout t ?text crlf))
	`))
	if err != nil {
		log.Fatal(err)
	}
	defer p.Close()

	for _, msg := range []string{"hello", "world"} {
		if err := p.Reset(); err != nil {
			log.Fatal(err)
		}
		if _, err := p.AssertFact("msg", ferric.Symbol(msg)); err != nil {
			log.Fatal(err)
		}
		if _, err := p.Run(context.Background()); err != nil {
			log.Fatal(err)
		}
		out, _ := p.GetOutput("t")
		fmt.Print(out)
		p.ClearOutput("t")
	}
	// Output:
	// hello
	// world
}

// ExamplePinnedEngine_Do demonstrates the Do escape hatch on PinnedEngine.
// Do gives you atomic, multi-step access to the underlying Engine within a
// single dispatched closure — useful when you need several operations to
// execute without interleaving from concurrent callers.
func ExamplePinnedEngine_Do() {
	p, err := ferric.NewPinnedEngine(ferric.WithSource(`
		(defglobal ?*total* = 0)
		(defrule sum
			(value ?v)
			=>
			(bind ?*total* (+ ?*total* ?v)))
	`))
	if err != nil {
		log.Fatal(err)
	}
	defer p.Close()

	err = p.Do(context.Background(), func(e *ferric.Engine) error {
		// Multiple operations execute atomically on the engine thread.
		for _, v := range []int64{10, 20, 30} {
			if _, err := e.AssertFact("value", v); err != nil {
				return err
			}
		}
		if _, err := e.Run(context.Background()); err != nil {
			return err
		}

		total, err := e.GetGlobal("total")
		if err != nil {
			return err
		}
		fmt.Println("total:", total)
		return nil
	})
	if err != nil {
		log.Fatal(err)
	}
	// Output:
	// total: 60
}

// ExamplePinnedEngine_introspection shows how to inspect rules, facts,
// and globals on a PinnedEngine.
func ExamplePinnedEngine_introspection() {
	p, err := ferric.NewPinnedEngine(ferric.WithSource(`
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
	defer p.Close()

	// Introspect before running.
	rules := p.Rules()
	fmt.Println("rules:", len(rules))
	for _, r := range rules {
		fmt.Printf("  %s (salience %d)\n", r.Name, r.Salience)
	}

	templates := p.Templates()
	// Filter to user-defined templates (exclude internal initial-fact).
	var userTemplates []string
	for _, t := range templates {
		if t != "initial-fact" {
			userTemplates = append(userTemplates, t)
		}
	}
	sort.Strings(userTemplates)
	fmt.Println("templates:", strings.Join(userTemplates, ", "))

	threshold, _ := p.GetGlobal("threshold")
	fmt.Println("threshold:", threshold)
	// Output:
	// rules: 1
	//   check-sensor (salience 0)
	// templates: sensor
	// threshold: 50
}
