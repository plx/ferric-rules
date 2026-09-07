; A fixed-width ordered pattern excludes shorter and longer facts.
;; Level: basic
;; Covers: facts, ordered-exact-arity
; Protocol: load, reset, run to quiescence.
(deffacts input (row a) (row a b) (row a b c))
(defrule observe (row ?a ?b) => (printout t ?a " " ?b crlf))
