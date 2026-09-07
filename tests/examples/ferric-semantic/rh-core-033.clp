; RH-CORE-033: invalid typed assertion in a rejected rule does not damage a valid consumer.
(deftemplate counter (slot n (type INTEGER)))
(deffacts seed (counter (n 5)))
(defrule valid (counter (n ?n)) => (printout t "valid " ?n crlf) (assert (result ?n)))
