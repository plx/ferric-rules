(defmodule A (export ?ALL))
(deftemplate A::foo (slot x))
(defmodule B (import A ?ALL))
(defrule B::rule1 (foo (x 3)) =>)
