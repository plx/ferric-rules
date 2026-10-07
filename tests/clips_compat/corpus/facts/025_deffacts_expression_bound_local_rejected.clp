;; A local bind result is valid, but deffacts cannot subsequently read that local.
;; Level: interaction
;; Covers: assertion-expression, deffacts, bind, variable-scope
(deffacts seed (row (bind ?x 3) ?x))
