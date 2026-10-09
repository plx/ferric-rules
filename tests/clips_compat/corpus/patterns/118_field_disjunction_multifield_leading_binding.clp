;; A leading multifield binding is shared by predicate alternatives and the RHS.
;; Level: interaction
;; Covers: patterns, field-disjunction, multifield, predicate-constraint, variable-binding, join
(deffacts seed (limit 2) (tags) (tags a) (tags a b c) (tags a b c d e))
(defrule pick
  (limit ?n)
  (tags $?t&:(< (length$ ?t) ?n)|:(> (length$ ?t) (+ ?n 2)))
  => (printout t "pick " (length$ ?t) " " ?t crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
