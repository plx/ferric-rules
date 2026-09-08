;; Issue #326: query traversal follows fact assertion order.
;; Before running: reset, host-retract item10, host-assert item5.
;; Run, reset, then run again; the golden concatenates both outputs.
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defrule probe =>
  (do-for-all-facts ((?f item)) TRUE
    (printout t (fact-slot-value ?f value) crlf))
  (do-for-fact ((?f item)) TRUE
    (printout t "first:" (fact-slot-value ?f value) crlf)))
