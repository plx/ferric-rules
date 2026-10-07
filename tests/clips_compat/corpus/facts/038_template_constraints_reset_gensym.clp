(deftemplate sample (slot id (default-dynamic (gensym*))))
(deffacts seed (sample))
(defrule show (sample (id ?id)) => (printout t ?id crlf))
