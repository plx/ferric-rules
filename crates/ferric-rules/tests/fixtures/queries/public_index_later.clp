;; Issue #329: user-visible fact assertion indices.
(deftemplate item (slot value))
(deffacts seed (padding) (item (value 10)) (unrelated) (item (value 20)))
(defrule probe =>
  (do-for-fact ((?f item)) (= (fact-slot-value ?f value) 10)
    (printout t "10:" (fact-index ?f) crlf))
  (do-for-fact ((?f item)) (= (fact-slot-value ?f value) 20)
    (printout t "20:" (fact-index ?f) crlf)))
