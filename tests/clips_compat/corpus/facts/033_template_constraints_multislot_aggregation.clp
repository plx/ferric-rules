(deftemplate sample
 (multislot fixed (type INTEGER) (cardinality 3 4) (default (create$ 1 2) 3))
 (multislot dynamic (type INTEGER) (cardinality 3 4) (default-dynamic (create$ 4) (create$ 5 6)))
 (multislot supplied (type INTEGER) (cardinality 3 4)))
(defrule run =>
 (bind ?f (assert (sample (supplied (create$ 7 8) (create$) 9))))
 (printout t (fact-slot-value ?f fixed) ":" (fact-slot-value ?f dynamic) ":" (fact-slot-value ?f supplied) crlf))
