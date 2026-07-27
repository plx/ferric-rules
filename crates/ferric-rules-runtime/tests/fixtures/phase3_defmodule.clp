;; Phase 3 fixture: defmodule
;; Demonstrates module declaration, focus-based execution, and rule isolation.

(defrule start
   (begin)
   =>
   (focus COUNTER)
   (assert (started)))

(defmodule COUNTER)

(defrule count-step
   (count-trigger)
   =>
   (assert (counted)))

(deffacts startup
   (begin)
   (count-trigger))
