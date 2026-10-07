;; A single slot rejects a statically known singleton multifield expression.
;; Level: interaction
;; Covers: assertion-expression, deffacts, deftemplate, create$
(deftemplate item (slot n))
(deffacts seed (item (n (create$ 1))))
