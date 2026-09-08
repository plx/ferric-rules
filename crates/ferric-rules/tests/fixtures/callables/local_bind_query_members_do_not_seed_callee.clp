(deftemplate item (slot value))
(deffacts seed (item (value 10)))
(deffunction isolated () (if FALSE then (bind ?f 0)) ?f)
(defrule probe =>
  (printout t (any-factp ((?f item)) (eq (isolated) ?f)) crlf)
  (printout t "after-error" crlf))
