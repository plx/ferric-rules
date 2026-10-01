(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)))
(defrule probe =>
 (do-for-fact ((?f item)) TRUE
  (retract ?f)
  (printout t "before:" ?f:value crlf)
  (return)
  (printout t "inside-after" crlf))
 (printout t "outside-after" crlf))
