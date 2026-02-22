(defmodule A (export ?ALL))
(defmodule B (import A ?ALL))
(defrule A::a-rule (initial-fact) => (printout t "A" crlf))
(defrule B::b-rule (initial-fact) => (printout t "B" crlf))
