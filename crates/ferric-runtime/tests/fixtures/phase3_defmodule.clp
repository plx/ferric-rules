;; Phase 3 fixture: defmodule
;; Demonstrates module declaration, focus-based execution, and rule isolation.

(defmodule COUNTER)

(defrule count-step
   (count-trigger)
   =>
   (assert (counted)))

(defmodule MAIN)

(defrule start
   (begin)
   =>
   (focus COUNTER)
   (assert (started)))

(deffacts startup
   (begin)
   (count-trigger))
