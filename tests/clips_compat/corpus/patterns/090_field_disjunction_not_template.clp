;; A disjunct blocks a negated template slot condition.
;; Level: interaction
;; Covers: patterns, field-disjunction, not, deftemplate
(deftemplate s (slot v))
(deffacts seed (go) (s (v a)))
(defrule match (go) (not (s (v a|b))) => (printout t "match" crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
