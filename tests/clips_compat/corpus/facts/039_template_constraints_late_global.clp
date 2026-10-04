(deftemplate sample (slot n (default-dynamic ?*later*)))
(defglobal ?*later* = 7)
(deffacts seed (sample))
(defrule show (sample (n ?n)) => (printout t ?n crlf))
