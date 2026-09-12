;; fact-index converts an address to its user-visible assertion index.
;; Level: basic
;; Covers: queries, fact-index-address
(deffacts seed (sample a))
(defrule probe ?f <- (sample a) => (printout t (fact-index ?f) crlf))
