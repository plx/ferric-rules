(deftemplate p (slot v))
(deffacts seed (p (v 1)))
(deffunction mutate (?t)
  (printout t "in-callable" crlf)
  (modify ?t (v 3))
  (printout t "unexpected-callable" crlf)
  TRUE)
(defrule probe (p (v 1)) =>
  (printout t "before " (mutate -1) " continued" crlf)
  (printout t "unexpected" crlf))
(defrule later (declare (salience -10)) =>
  (printout t "later" crlf))
