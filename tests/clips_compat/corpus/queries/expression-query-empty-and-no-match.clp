
(deftemplate item (slot value))
(deftemplate empty-item (slot value))
(deffacts seed (item (value 10)))
(defrule probe =>
  (printout t "empty:"
    (any-factp ((?f empty-item)) TRUE) ":"
    (length$ (find-fact ((?f empty-item)) TRUE)) ":"
    (length$ (find-all-facts ((?f empty-item)) TRUE)) crlf)
  (printout t "filtered:"
    (any-factp ((?f item)) (> ?f:value 20)) ":"
    (length$ (find-fact ((?f item)) (> ?f:value 20))) ":"
    (length$ (find-all-facts ((?f item)) (> ?f:value 20))) crlf)
  (printout t "unevaluated:" (any-factp ((?f empty-item)) ?f:missing) crlf))
