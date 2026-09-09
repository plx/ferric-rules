;; Issue #329: user-visible fact assertion indices.
(deftemplate item (slot value))
(deffacts seed (item (value 10)))
(defrule probe =>
  (do-for-fact ((?f item)) TRUE (printout t (fact-index ?f) crlf)))
