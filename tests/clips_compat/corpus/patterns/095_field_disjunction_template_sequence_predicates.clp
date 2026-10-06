;; Overlapping predicates produce one match per bound multislot element.
;; Level: interaction
;; Covers: patterns, field-disjunction, deftemplate, multislot, predicate-constraint, variable-binding
(deftemplate item (multislot tags))
(deffacts seed (item (tags -2 5 12)))
(defrule match (item (tags $? ?t&:(> ?t 0)|:(< ?t 10) $?))
  => (printout t "P " ?t crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
