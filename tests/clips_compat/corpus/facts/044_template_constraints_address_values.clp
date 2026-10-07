(deftemplate target)
(deftemplate sample (slot x (allowed-values a)))
(deftemplate dummy-source (slot x (type FACT-ADDRESS)))
(defrule run =>
 (bind ?target (assert (target)))
 (bind ?dummy (assert (dummy-source)))
 (bind ?a (assert (sample (x ?target))))
 (bind ?b (assert (sample (x (fact-slot-value ?dummy x)))))
 (printout t (fact-slot-value ?a x) ":" (fact-slot-value ?b x) crlf))
