;; Issue #329: user-visible fact assertion indices.
(deftemplate item (slot value))
(defrule report =>
  (do-for-fact ((?f item)) TRUE (printout t (fact-index ?f) crlf)))
