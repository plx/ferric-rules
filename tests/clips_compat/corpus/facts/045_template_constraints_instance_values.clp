(deftemplate sample (slot x (type INSTANCE-NAME) (allowed-values [first] [second])))
(defrule run =>
 (bind ?a (assert (sample)))
 (bind ?b (assert (sample (x [second]))))
 (printout t (fact-slot-value ?a x) ":" (fact-slot-value ?b x) crlf))
