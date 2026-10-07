(deftemplate sample (slot required (allowed-symbols ready) (default ?NONE)))
(deffacts seed (sample (required ready)))
(defrule show (sample (required ?v)) => (printout t ?v crlf))
