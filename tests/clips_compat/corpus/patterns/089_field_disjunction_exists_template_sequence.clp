;; An existential multislot match counts witnesses across alternatives once.
;; Level: interaction
;; Covers: patterns, field-disjunction, exists, deftemplate, multislot
(deftemplate item (multislot tags))
(deffacts seed (go) (item (tags a b)))
(defrule match (go) (exists (item (tags $? a|b $?)))
  => (printout t "match" crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
