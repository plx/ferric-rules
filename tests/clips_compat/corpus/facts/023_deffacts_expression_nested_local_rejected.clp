;; A local variable inside a seed function call also fails load.
;; Level: interaction
;; Covers: assertion-expression, deffacts, variable-scope
(deffacts seed (value (+ 1 ?missing)))
