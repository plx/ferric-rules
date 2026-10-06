;; Variables introduced only in alternatives are not available for later joins.
;; Level: boundary
;; Covers: patterns, field-disjunction, deftemplate, variable-binding
(deftemplate mnj (slot x) (slot y))
(defrule t (mnj (x ?x|?y) (y ?x|?y)) =>)
