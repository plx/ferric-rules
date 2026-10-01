(deftemplate item (slot value))
(deffacts seed (item (value 10)))
(defrule probe =>
  (bind ?found (find-all-facts ((?f item)) ?f:missing))
  (printout t "unexpected" crlf))
