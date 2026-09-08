
(deftemplate item (slot value))
(deffacts seed (item (value 10)))
(defrule probe =>
  (printout t
    (any-factp ((?f item)) (progn (bind ?unrelated 42) TRUE)) crlf))
