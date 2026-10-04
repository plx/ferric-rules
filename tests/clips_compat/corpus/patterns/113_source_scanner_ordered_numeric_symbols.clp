;; Numeric-looking symbols remain one ordered field in source facts and patterns.
;; Level: boundary
;; Covers: source-scanner, deffacts, ordered-facts, explode$, multifield
(deffacts seed (place 1st 5th 2nd) (numbers 1. .5 0x10 12abc))
(defrule symbols (declare (salience 10))
  (place $?values)
  (place 1st 5th 2nd)
  => (printout t "symbols: " (length$ ?values) " " ?values crlf)
  (printout t "scanned: " (explode$ "1st 5th 2nd") crlf))
(defrule numbers
  (numbers $?values)
  => (printout t "facts: " (length$ ?values) " " ?values crlf)
  (printout t "source: " (create$ 1. .5 0x10 12abc) crlf)
  (printout t "scanned: " (explode$ "1. .5 0x10 12abc") crlf))
