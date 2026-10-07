(defrule r =>
  (printout t "fire" crlf)
  (halt)
  (reset)
  (printout t "after" crlf))
(defrule s (declare (salience -10)) =>
  (printout t "unexpected-s" crlf))
