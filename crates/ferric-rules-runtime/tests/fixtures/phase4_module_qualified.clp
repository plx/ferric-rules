;; Phase 4: Module-qualified name resolution and visibility fixture.
;; Exercises MODULE::name resolution for functions.

(defmodule MATH (export ?ALL))

(deffunction add (?a ?b)
    (+ ?a ?b))

(deffunction square (?x)
    (* ?x ?x))

(defmodule MAIN (import MATH ?ALL))

(defrule use-math
    (compute ?x ?y)
    =>
    (printout t "add: " (MATH::add ?x ?y) crlf)
    (printout t "square: " (MATH::square ?x) crlf))

(deffacts startup (compute 3 4))
