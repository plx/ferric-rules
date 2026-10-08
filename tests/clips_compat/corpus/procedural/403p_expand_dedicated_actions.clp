; Commands with their own RHS handlers accept expand$ operands.
(deffacts seed (a))
(defrule old (declare (salience -10)) => (printout t "old fired" crlf))
(defrule r (declare (salience 10)) (a)
  =>
  (undefrule (expand$ (create$ old)))
  (undeffacts (expand$ (create$ seed)))
  (printout t "after" crlf))
(defrule done (declare (salience -20)) => (printout t "done" crlf))
