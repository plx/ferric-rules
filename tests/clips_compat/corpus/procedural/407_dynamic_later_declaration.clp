(defrule query => (printout t (length$ (find-all-facts ((?f (sym-cat later))) TRUE)) crlf))
(deffacts d (later 1))
