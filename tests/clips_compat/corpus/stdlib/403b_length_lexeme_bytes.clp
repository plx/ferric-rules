(defrule test => (printout t (length abc) "|" (length$ abc) "|" (length "héllo") "|" (length$ "héllo") "|" (length (create$ a b c)) "|" (length$ (create$)) crlf))
