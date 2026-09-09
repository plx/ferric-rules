;; Query slot references select facts using their slot values.
;; Level: boundary
;; Covers: queries, any-factp-filter
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defrule probe =>
    (printout t (any-factp ((?f item)) (> ?f:value 20)) ":" (any-factp ((?f item)) (> ?f:value 40)) crlf))