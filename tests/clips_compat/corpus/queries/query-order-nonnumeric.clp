;; Issue #326: query traversal follows fact assertion order.
(deftemplate item (slot value))
(deffacts seed (item (value zulu)) (item (value alpha)) (item (value middle)))
(defrule probe =>
  (do-for-all-facts ((?f item)) TRUE
    (printout t (fact-slot-value ?f value) crlf)))
