;; A colon-named local is not a rebinding of the query member itself.
(deftemplate item (slot value))
(deffacts seed (item (value 10)))
(defrule probe =>
  (do-for-all-facts ((?f item)) TRUE
    (bind ?f:value 99)
    (printout t ?f:value crlf)))
