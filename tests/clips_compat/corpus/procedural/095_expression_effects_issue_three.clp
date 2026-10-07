(deftemplate p (slot v))
(deffacts d (p (v 1)))
(defrule a (declare (salience 10)) => (printout t "a" crlf))
(defrule b (p (v 1)) =>
  (do-for-fact ((?x p)) TRUE (printout t "body" crlf) (reset) (printout t "body2" crlf))
  (printout t "after" crlf)
  (halt))
