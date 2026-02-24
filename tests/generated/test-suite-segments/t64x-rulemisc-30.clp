(deftemplate pay
   (slot code)
   (slot processed))
(deffacts initial-data
   (pay (code A) (processed 1))
   (pay (code A) (processed 2)))
(defrule Secondary ""
   ?p <- (pay (processed ~TRUE))
   =>
   (modify ?p (processed TRUE)))
