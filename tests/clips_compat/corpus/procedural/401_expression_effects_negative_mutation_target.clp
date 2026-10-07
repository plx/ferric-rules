(deftemplate p (slot v))
(deffacts seed (p (v 1)))
(defrule probe (p (v 1)) =>
  (bind ?t -1)
  (printout t "before " (duplicate ?t (v 4)) " continued" crlf)
  (printout t "unexpected" crlf))
(defrule later (declare (salience -10)) =>
  (printout t "later" crlf))
