; An RHS progn body accepts action-only commands, as if and while bodies do.
(deffacts seed (a))
(defrule other (declare (salience -10)) => (printout t "other fired" crlf))
(defrule r (declare (salience 10)) (a)
  =>
  (progn (undeffacts seed) (undefrule other))
  (printout t "ok" crlf))
(defrule done (declare (salience -20)) => (printout t "done" crlf))
