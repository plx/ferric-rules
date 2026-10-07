(defrule high (declare (salience (progn (bind ?x 9) (loop-for-count 2 (break)) ?x)))
  => (printout t high crlf))
(defrule low (declare (salience 8)) => (printout t low crlf))
