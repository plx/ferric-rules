;; initial-fact has no implied slot, so reading one is an error that stops the
;; rule and the run.
;; Level: boundary
;; Covers: queries, fact-slot-value, initial-fact
(defrule probe (declare (salience 10)) =>
  (printout t "before" crlf)
  (bind ?v (fact-slot-value 0 implied))
  (printout t "not reached " ?v crlf))
(defrule later => (printout t "not reached" crlf))
