(deffunction check-input (?last ?input)
    (if (and (>= ?input 1) (<= ?input ?last)) 
        then
        (return TRUE)
        else
        (return FALSE)
    )
)

(deftemplate domanda
    (slot num-domanda
        (type INTEGER)
    )
    (slot num-risposta
        (type INTEGER)
    )
)

(deftemplate fase
    (slot num
        (type INTEGER)
    )
)

(deftemplate causa
    (slot risultato
        (type SYMBOL)
        (allowed-symbols ritardo-semplice fono-articolatorio sordita-ipoacusia spettro-autistico ritardo-mentale disfasia-espressiva disfasia-mista)
    )
)

(defrule inizio-casuale
    (declare (salience 1))
    =>
    (printout t "SCREENING INIZIALE DELLE CAUSE DI UN DEFICIT DI LINGUAGGIO IN ETA'PRESCOLARE" crlf crlf)
    (assert (causa (risultato ritardo-semplice)))
    (assert (causa (risultato fono-articolatorio)))
    (assert (causa (risultato sordita-ipoacusia)))
    (assert (causa (risultato spettro-autistico)))
    (assert (causa (risultato ritardo-mentale)))
    (assert (causa (risultato disfasia-espressiva)))
    (assert (causa (risultato disfasia-mista)))
    (assert (fase (num 1)))
    (set-strategy random)
)

(defrule domanda-9
    (fase (num 1))
    (not (eta ?eta))
    =>
    (printout t "9. Il bambino ha:" crlf)
	(printout t "   1) Meno di 3 anni" crlf)
	(printout t "   2) Piu' di 3 anni" crlf)
	(printout t "   3) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 2 ?risposta) FALSE)
		(if (eq ?risposta 3)
            then
            (printout t "Deve indicare l'eta' attuale del bambino." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 9) (num-risposta ?risposta)))
)

(defrule eta-meno-3
    (declare (salience 1))
    ?risp <- (domanda (num-domanda 9) (num-risposta 1))
    (fase (num 1))
    =>
    (retract ?risp)
    (assert (eta meno-3))
)

(defrule eta-piu-3
    (declare (salience 1))
    ?risp <- (domanda (num-domanda 9) (num-risposta 2))
    (fase (num 1))
    =>
    (retract ?risp)
    (assert (eta piu-3))
)

(defrule domanda-1
    (fase (num 1))
    (not (parlato ?parlato))
    =>
	(printout t "1. Come parla adesso il bambino?" crlf)
	(printout t "   1) Non dice quasi nulla" crlf)
	(printout t "   2) Parla, ma in misura scarsa rispetto ai suoi coetanei" crlf)
	(printout t "   3) Parla, ma non pronuncia bene alcuni o molti suoni" crlf)
	(printout t "   4) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 3 ?risposta) FALSE)
		(if (eq ?risposta 4)
            then
            (printout t "Deve indicare come il bambino, attualmente, si esprime, ad esempio se non dice nessuna parola o ne dice poche oppure ne dice tante ma incomprensibili." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 1) (num-risposta ?risposta)))
)

(defrule parlato-quasi-nulla
    (declare (salience 1))
    ?risp <- (domanda (num-domanda 1) (num-risposta 1))
    (fase (num 1))
    =>
    (retract ?risp)
    (assert (parlato quasi-nulla))
)

(defrule parlato-scarso
    (declare (salience 1))
    ?risp <- (domanda (num-domanda 1) (num-risposta 2))
    (fase (num 1))
    =>
    (retract ?risp)
    (assert (parlato scarso))
)

(defrule parlato-mal-pronuncia
    (declare (salience 1))
    ?risp <- (domanda (num-domanda 1) (num-risposta 3))
    (fase (num 1))
    =>
    (retract ?risp)
    (assert (parlato mal-pronuncia))
)

(defrule domanda-2
    (fase (num 1))
    (not (comprende ?comprende))
    =>
	(printout t "2. Il bambino comprende cio' che gli si dice?" crlf)
	(printout t "   1) Si, bene" crlf)
	(printout t "   2) Parzialmente" crlf)
	(printout t "   3) Sembra comprendere alcune cose ma non altre" crlf)
	(printout t "   4) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 3 ?risposta) FALSE)
		(if (eq ?risposta 4)
            then
            (printout t "Deve indicare in che misura il bambino comprende gli ordini che gli vengono impartiti, ad esempio se lo capisce subito oppure dopo qualche volta e con aiuto." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 2) (num-risposta ?risposta)))
)

(defrule comprende-si
    (declare (salience 1))
    ?risp <- (domanda (num-domanda 2) (num-risposta 1))
    (fase (num 1))
    =>
    (retract ?risp)
    (assert (comprende si))
)

(defrule comprende-parzialmente
    (declare (salience 1))
    ?risp <- (domanda (num-domanda 2) (num-risposta 2))
    (fase (num 1))
    =>
    (retract ?risp)
    (assert (comprende parzialmente))
)

(defrule comprende-non-tutto
    (declare (salience 1))
    ?risp <- (domanda (num-domanda 2) (num-risposta 3))
    (fase (num 1))
    =>
    (retract ?risp)
    (assert (comprende non-tutto))
)

(defrule inizia-diagnosi
    (declare (salience 1))
    (fase (num 1))
    (comprende ?comprende)
    (parlato ?eta)
    =>
    (set-strategy simplicity)
)

(defrule ipotizza-ritardo-semplice
    ?fonoart <- (causa (risultato fono-articolatorio))
    ?sordita <- (causa (risultato sordita-ipoacusia))
    ?espressiva <- (causa (risultato disfasia-espressiva))
    (eta meno-3)
    (or (parlato quasi-nulla) (parlato scarso) (parlato mal-pronuncia))
    (comprende si)
    =>
    (retract ?fonoart)
    (retract ?sordita)
    (retract ?espressiva)
    (assert (ipotesi ritardo-semplice))
)

(defrule ipotizza-fono-articolatorio
    ?ritsem <- (causa (risultato ritardo-semplice))
    ?sordita <- (causa (risultato sordita-ipoacusia))
    ?espressiva <- (causa (risultato disfasia-espressiva))
    (eta piu-3)
    (parlato mal-pronuncia)
    (comprende si)
    =>
    (retract ?ritsem)
    (retract ?sordita)
    (retract ?espressiva)
    (assert (ipotesi fono-articolatorio))
)

(defrule ipotizza-disfasia-espressiva
    ?ritsem <- (causa (risultato ritardo-semplice))
    ?fonoart <- (causa (risultato fono-articolatorio))
    ?sordita <- (causa (risultato sordita-ipoacusia))
    (eta piu-3)
    (or (parlato quasi-nulla) (parlato scarso)) 
    (comprende si)
    =>
    (retract ?ritsem)
    (retract ?fonoart)
    (retract ?sordita)
    (assert (ipotesi disfasia-espressiva))
)

(defrule ipotizza-sordita-ipoacusia
    ?ritsem<- (causa (risultato ritardo-semplice))
    ?fonoart <- (causa (risultato fono-articolatorio))
    ?espressiva <- (causa (risultato disfasia-espressiva))
    (or (eta meno-3) (eta piu-3))
    (or (parlato quasi-nulla) (parlato scarso) (parlato mal-pronuncia))
    (or (comprende parzialmente) (comprende non-tutto))
    =>
    (retract ?ritsem)
    (retract ?fonoart)
    (retract ?espressiva)
    (assert (ipotesi sordita-ipoacusia))
)

(defrule domanda-4
    (ipotesi sordita-ipoacusia)
    (fase (num 1))
    (not (reagisce-richiami-se-guarda ?reagisce))
    =>
    (printout t "4. Il bambino comprende/reagisce ai richiami piu' facilmente se ha la possibilita' di guardare in viso chi parla?" crlf)
	(printout t "   1) Si" crlf)
	(printout t "   2) No" crlf)
	(printout t "   3) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 2 ?risposta) FALSE)
		(if (eq ?risposta 3)
            then
            (printout t "Deve indicare se il bambino si aiuta nella comprensione guardando le espressioni del viso e il movimento delle labbra." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 4) (num-risposta ?risposta)))
)

(defrule reagisce-richiami-se-guarda-si
    (declare (salience 1))
    (fase (num 1))
    ?risp <- (domanda (num-domanda 4) (num-risposta 1))
    =>
    (retract ?risp)
    (assert (reagisce-richiami-se-guarda si))
)

(defrule reagisce-richiami-se-guarda-no
    (declare (salience 1))
    (fase (num 1))
    ?risp <- (domanda (num-domanda 4) (num-risposta 2))
    =>
    (retract ?risp)
    (assert (reagisce-richiami-se-guarda no))
)

(defrule domanda-5
    (ipotesi sordita-ipoacusia)
    (fase (num 1))
    (reagisce-richiami-se-guarda si)
    (not (reagisce-suoni ?reagisce))
    =>
	(printout t "5. Il bambino reagisce ad altri suoni?" crlf)
	(printout t "   1) Si" crlf)
	(printout t "   2) No" crlf)
	(printout t "   3) Non sempre" crlf)
	(printout t "   4) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 3 ?risposta) FALSE)
		(if (eq ?risposta 4)
            then
            (printout t "Deve indicare in che misura il bambino sente altri suoni come ad esempio il telefono, il campanello, la TV, una porta che sbatte ecc." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 5) (num-risposta ?risposta)))
)

(defrule reagisce-suoni-si
    (declare (salience 1))
    (fase (num 1))
    ?risp <- (domanda (num-domanda 5) (num-risposta 1))
    =>
    (retract ?risp)
    (assert (reagisce-suoni si))
)

(defrule reagisce-suoni-no
    (declare (salience 1))
    (fase (num 1))
    ?risp <- (domanda (num-domanda 5) (num-risposta 2))
    =>
    (retract ?risp)
    (assert (reagisce-suoni no))
)

(defrule reagisce-suoni-non-sempre
    (declare (salience 1))
    (fase (num 1))
    ?risp <- (domanda (num-domanda 5) (num-risposta 3))
    =>
    (retract ?risp)
    (assert (reagisce-suoni non-sempre))
)

(defrule domanda-8-caso-1
    (ipotesi ritardo-semplice)
    (fase (num 1))
    (not (altre-difficolta ?altre))
    =>
    (printout t "8. Il bambino mostra di avere altre difficolta' oltre quelle linguistiche?" crlf)
	(printout t "   1) Si" crlf)
	(printout t "   2) No" crlf)
	(printout t "   3) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 2 ?risposta) FALSE)
		(if (eq ?risposta 3)
            then
            (printout t "Deve indicare se il bambino dimostra altre incapacita' e/o difficolta' oltre a quelle linguistiche (es. comportamentali, nelle autonomie, nelle funzioni fisiologiche ecc.) rispetto ai suoi coetanei." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 8) (num-risposta ?risposta)))
)

(defrule domanda-8-caso-2
    (ipotesi fono-articolatorio)
    (fase (num 1))
    (not (altre-difficolta ?altre))
    =>
    (printout t "8. Il bambino mostra di avere altre difficolta' oltre quelle linguistiche?" crlf)
	(printout t "   1) Si" crlf)
	(printout t "   2) No" crlf)
	(printout t "   3) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 2 ?risposta) FALSE)
		(if (eq ?risposta 3)
            then
            (printout t "Deve indicare se il bambino dimostra altre incapacita' e/o difficolta' oltre a quelle linguistiche (es. comportamentali, nelle autonomie, nelle funzioni fisiologiche ecc.) rispetto ai suoi coetanei." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 8) (num-risposta ?risposta)))
)

(defrule domanda-8-caso-3
    (ipotesi disfasia-espressiva)
    (fase (num 1))
    (not (altre-difficolta ?altre))
    =>
    (printout t "8. Il bambino mostra di avere altre difficolta' oltre quelle linguistiche?" crlf)
	(printout t "   1) Si" crlf)
	(printout t "   2) No" crlf)
	(printout t "   3) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 2 ?risposta) FALSE)
		(if (eq ?risposta 3)
            then
            (printout t "Deve indicare se il bambino dimostra altre incapacita' e/o difficolta' oltre a quelle linguistiche (es. comportamentali, nelle autonomie, nelle funzioni fisiologiche ecc.) rispetto ai suoi coetanei." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 8) (num-risposta ?risposta)))
)

(defrule altre-difficolta-si
    (declare (salience 1))
    (fase (num 1))
    ?risp <- (domanda (num-domanda 8) (num-risposta 1))
    =>
    (retract ?risp)
    (assert (altre-difficolta si))
)

(defrule altre-difficolta-no
    (declare (salience 1))
    ?risp <- (domanda (num-domanda 8) (num-risposta 2))
    =>
    (retract ?risp)
    (assert (altre-difficolta no))
)

(defrule fase-2-caso-1
    ?ipotesi <- (ipotesi sordita-ipoacusia)
    (reagisce-richiami-se-guarda no)
    ?fase <-  (fase (num 1))
    ?sordita <- (causa (risultato sordita-ipoacusia))
    =>
    (retract ?fase)
    (retract ?sordita)
    (assert (fase (num 2)))
    (retract ?ipotesi)
)

(defrule fase-2-caso-2
    ?ipotesi <- (ipotesi sordita-ipoacusia)
    (reagisce-richiami-se-guarda si)
    (reagisce-suoni si)
    ?fase <-  (fase (num 1))
    ?sordita <- (causa (risultato sordita-ipoacusia))
    =>
    (retract ?fase)
    (retract ?sordita)
    (assert (fase (num 2)))
    (retract ?ipotesi)
)

(defrule ipotizza-spettro-autistico
    (fase (num 2))
    (causa (risultato spettro-autistico))
    (and (not (ipotesi ritardo-mentale)) (not (ipotesi disfasia-mista)))
    =>
    (assert (ipotesi spettro-autistico))
)

(defrule ipotizza-ritardo-mentale
    (fase (num 2))
    (causa (risultato ritardo-mentale))
    (and (not (ipotesi disfasia-mista)) (not (ipotesi spettro-autistico)))
    =>
    (assert (ipotesi ritardo-mentale))
)

(defrule ipotizza-disfasia-mista
    (fase (num 2))
    (causa (risultato disfasia-mista))
    (and (not (ipotesi spettro-autistico)) (not (ipotesi ritardo-mentale)))
    =>
    (assert (ipotesi disfasia-mista))
)

(defrule domanda-3
    (fase (num 2))
    (ipotesi spettro-autistico)
    (not (risponde-nome ?risponde))
    =>
    (printout t "3. Se lo si chiama per nome, rivolge attenzione a chi lo chiama?" crlf)
	(printout t "   1) Si" crlf)
	(printout t "   2) No" crlf)
	(printout t "   3) Non sempre" crlf)
	(printout t "   4) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 3 ?risposta) FALSE)
		(if (eq ?risposta 4)
            then
            (printout t "Deve indicare in che misura il bambino si gira verso chi lo chiama per nome." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 3) (num-risposta ?risposta)))
)

(defrule risponde-nome-si
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 3) (num-risposta 1))
    =>
    (retract ?risp)
    (assert (risponde-nome si))
)

(defrule risponde-nome-no
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 3) (num-risposta 2))
    =>
    (retract ?risp)
    (assert (risponde-nome no))
)

(defrule risponde-nome-non-sempre
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 3) (num-risposta 3))
    =>
    (retract ?risp)
    (assert (risponde-nome non-sempre))
)

(defrule domanda-11
    (fase (num 2))
    (ipotesi spettro-autistico)
    (not (guarda-occhi ?guarda))
    =>
    (printout t "11. Il bambino guarda negli occhi le altre persone?" crlf)
	(printout t "   1) Si" crlf)
	(printout t "   2) Poco o per niente" crlf)
	(printout t "   3) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 2 ?risposta) FALSE)
		(if (eq ?risposta 3)
            then
            (printout t "Deve indicare se il bambino vi guarda negli occhi ad esempio quando gli si parla, quando gioca o quando piu' generalmente si interagisce con lui." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 11) (num-risposta ?risposta)))
)

(defrule guarda-occhi-si
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 11) (num-risposta 1))
    =>
    (retract ?risp)
    (assert (guarda-occhi si))
)

(defrule guarda-occhi-poco-niente
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 11) (num-risposta 2))
    =>
    (retract ?risp)
    (assert (guarda-occhi poco-niente))
)

(defrule domanda-13
    (fase (num 2))
    (ipotesi spettro-autistico)
    (not (interessato-gioco-altri ?interessato))
    =>
	(printout t "13. Il bambino e' interessato al gioco con altri bambini?" crlf)
	(printout t "   1) Si" crlf)
	(printout t "   2) Poco o per niente" crlf)
    (printout t "   3) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 2 ?risposta) FALSE)
		(if (eq ?risposta 3)
            then
            (printout t "Deve indicare se e in quale misura il bambino cerca altri bambini per giocare on loro oppure se si aggrega a loro se li vede giocare." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 13) (num-risposta ?risposta)))        
)

(defrule interessato-gioco-altri-si
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 13) (num-risposta 1))
    =>
    (retract ?risp)
    (assert (interessato-gioco-altri si))
)

(defrule interessato-gioco-altri-poco-niente
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 13) (num-risposta 2))
    =>
    (retract ?risp)
    (assert (interessato-gioco-altri poco-niente))
)

(defrule domanda-15
    (fase (num 2))
    (ipotesi spettro-autistico)
    (not (accetta-modifica-schemi ?accetta))
    =>
	(printout t "15. Mentre gioca, accetta che altri modifichino i suoi schemi?" crlf)
	(printout t "   1) Si" crlf)
	(printout t "   2) Poco o per niente" crlf)
    (printout t "   3) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 2 ?risposta) FALSE)
		(if (eq ?risposta 3)
            then
            (printout t "Deve indicare se il bambino, durante il suo gioco, si ribella se qualcuno cambia la posizione che lui ha dato ai giocattoli." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 15) (num-risposta ?risposta)))    
)

(defrule accetta-modifica-schemi-gioco-si
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 15) (num-risposta 1))
    =>
    (retract ?risp)
    (assert (accetta-modifica-schemi si))
)

(defrule accetta-modifica-schemi-gioco-poco-niente
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 15) (num-risposta 2))
    =>
    (retract ?risp)
    (assert (accetta-modifica-schemi poco-niente))
)

(defrule domanda-19-caso-1
    (fase (num 2))
    (ipotesi spettro-autistico)
    (not (abitudinario ?abitudinario))
    =>
	(printout t "19. E' abitudinario nel gioco e altre attivita'?" crlf)
	(printout t "   1) Si" crlf)
	(printout t "   2) No" crlf)
    (printout t "   3) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 2 ?risposta) FALSE)
		(if (eq ?risposta 3)
            then
            (printout t "Deve indicare se il bambino vuole fare le cose sempre nello stesso modo o provare sempre le stesse strade." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 19) (num-risposta ?risposta)))
)

(defrule domanda-19-caso-2
    (fase (num 2))
    (ipotesi disfasia-mista)
    (not (abitudinario ?abitudinario))
    =>
	(printout t "19. E' abitudinario nel gioco e altre attivita'?" crlf)
	(printout t "   1) Si" crlf)
	(printout t "   2) No" crlf)
    (printout t "   3) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 2 ?risposta) FALSE)
		(if (eq ?risposta 3)
            then
            (printout t "Deve indicare se il bambino vuole fare le cose sempre nello stesso modo o provare sempre le stesse strade." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 19) (num-risposta ?risposta)))
)

(defrule abitudinario-si
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 19) (num-risposta 1))
    =>
    (retract ?risp)
    (assert (abitudinario si))
)

(defrule abitudinario-no
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 19) (num-risposta 2))
    =>
    (retract ?risp)
    (assert (abitudinario no))
)

(defrule domanda-16
    (fase (num 2))
    (ipotesi ritardo-mentale)
    (conferma ritardo-mentale)
    (not (iperattivo-caotico-poco-attento ?iperattivo))
    =>
    (printout t "16. E' iperattivo/caotico/poco attento?" crlf)
	(printout t "   1) Si" crlf)
	(printout t "   2) Poco o per niente" crlf)
    (printout t "   3) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 2 ?risposta) FALSE)
		(if (eq ?risposta 3)
            then
            (printout t "Deve indicare se il bambino e' sempre in movimento, passa da un gioco all'altro molto rapidamente o si stanca di tutto." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 16) (num-risposta ?risposta)))
)

(defrule iperattivo-caotico-poco-attento-si
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 16) (num-risposta 1))
    =>
    (retract ?risp)
    (assert (iperattivo-caotico-poco-attento si))
)

(defrule iperattivo-caotico-poco-attento-poco-niente
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 16) (num-risposta 2))
    =>
    (retract ?risp)
    (assert (iperattivo-caotico-poco-attento poco-niente))
)

(defrule domanda-12-caso-3
    (fase (num 2))
    (ipotesi ritardo-mentale)
    (conferma ritardo-mentale)
    (not (esprime-desiderio ?esprime))
    =>
	(printout t "12. Il bambino esprime il suo bisogno o desiderio di un oggetto:" crlf)
	(printout t "   1) Indicando" crlf)
	(printout t "   2) Usando anche altri tipi di gesti" crlf)
	(printout t "   3) Portando la mano dell'altro verso l'oggetto" crlf)
    (printout t "   4) Piangendo e/o agitandosi" crlf)
    (printout t "   5) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 4 ?risposta) FALSE)
		(if (eq ?risposta 5)
            then
            (printout t "Deve indicare con quali modalita' il bambino indica il suo bisogno o desiderio di un oggetto, ad esempio, puntando il dito indice o usando altri gesti, piangendo o pretendendo che l'adulto prenda l'oggetto per lui." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 12) (num-risposta ?risposta)))
)

(defrule domanda-12-caso-2
    (fase (num 2))
    (ipotesi spettro-autistico)
    (conferma spettro-autistico)
    (not (esprime-desiderio ?esprime))
    =>
	(printout t "12. Il bambino esprime il suo bisogno o desiderio di un oggetto:" crlf)
	(printout t "   1) Indicando" crlf)
	(printout t "   2) Usando anche altri tipi di gesti" crlf)
	(printout t "   3) Portando la mano dell'altro verso l'oggetto" crlf)
    (printout t "   4) Piangendo e/o agitandosi" crlf)
    (printout t "   5) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 4 ?risposta) FALSE)
		(if (eq ?risposta 5)
            then
            (printout t "Deve indicare con quali modalita' il bambino indica il suo bisogno o desiderio di un oggetto, ad esempio, puntando il dito indice o usando altri gesti, piangendo o pretendendo che l'adulto prenda l'oggetto per lui." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 12) (num-risposta ?risposta)))
)

(defrule domanda-12-caso-1
    (fase (num 2))
    (ipotesi disfasia-mista)
    (not (esprime-desiderio ?esprime))
    =>
	(printout t "12. Il bambino esprime il suo bisogno o desiderio di un oggetto:" crlf)
	(printout t "   1) Indicando" crlf)
	(printout t "   2) Usando anche altri tipi di gesti" crlf)
	(printout t "   3) Portando la mano dell'altro verso l'oggetto" crlf)
    (printout t "   4) Piangendo e/o agitandosi" crlf)
    (printout t "   5) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 4 ?risposta) FALSE)
		(if (eq ?risposta 5)
            then
            (printout t "Deve indicare con quali modalita' il bambino indica il suo bisogno o desiderio di un oggetto, ad esempio, puntando il dito indice o usando altri gesti, piangendo o pretendendo che l'adulto prenda l'oggetto per lui." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 12) (num-risposta ?risposta)))
)

(defrule esprime-desiderio-indicando
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 12) (num-risposta 1))
    =>
    (retract ?risp)
    (assert (esprime-desiderio indicando))
)

(defrule esprime-desiderio-gesti
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 12) (num-risposta 2))
    =>
    (retract ?risp)
    (assert (esprime-desiderio gesti))
)

(defrule esprime-desiderio-mano-dell-altro
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 12) (num-risposta 3))
    =>
    (retract ?risp)
    (assert (esprime-desiderio mano-dell-altro))
)

(defrule esprime-desiderio-piangendo
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 12) (num-risposta 4))
    =>
    (retract ?risp)
    (assert (esprime-desiderio piangendo))
)

(defrule domanda-18-caso-1
    (fase (num 2))
    (ipotesi spettro-autistico)
    (conferma spettro-autistico)
    (not (resistenze-spavento-luoghi ?resistenze))
    =>
	(printout t "18. Ha resistenze o si spaventa per luoghi non familiari, rumori e/o oggetti?" crlf)
	(printout t "   1) Si" crlf)
	(printout t "   2) No" crlf)
    (printout t "   3) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 2 ?risposta) FALSE)
		(if (eq ?risposta 3)
            then
            (printout t "Deve indicare se il bambino si spaventa a causa di posti, rumori, oggetti che di solito piacciono agli altri bambini." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 18) (num-risposta ?risposta)))
)

(defrule domanda-18-caso-2
    (fase (num 2))
    (ipotesi disfasia-mista)
    (not (resistenze-spavento-luoghi ?resistenze))
    =>
	(printout t "18. Ha resistenze o si spaventa per luoghi non familiari, rumori e/o oggetti?" crlf)
	(printout t "   1) Si" crlf)
	(printout t "   2) No" crlf)
    (printout t "   3) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 2 ?risposta) FALSE)
		(if (eq ?risposta 3)
            then
            (printout t "Deve indicare se il bambino si spaventa a causa di posti, rumori, oggetti che di solito piacciono agli altri bambini." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 18) (num-risposta ?risposta)))
)

(defrule resistenze-spavento-luoghi-si
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 18) (num-risposta 1))
    =>
    (retract ?risp)
    (assert (resistenze-spavento-luoghi si))
)

(defrule resistenze-spavento-luoghi-no
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 18) (num-risposta 2))
    =>
    (retract ?risp)
    (assert (resistenze-spavento-luoghi no))
)

(defrule domanda-14-caso-1
    (fase (num 2))
    (ipotesi disfasia-mista)
    (not (gioco-oggetti ?gioco))
    =>
	(printout t "14. Il gioco con gli oggetti e':" crlf)
	(printout t "   1) Strettamente legato alla funzione dell'oggetto" crlf)
	(printout t "   2) Di tipo simbolico" crlf)
    (printout t "   3) Basato sulla ricerca di effetti sensoriali" crlf)
    (printout t "   4) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 3 ?risposta) FALSE)
		(if (eq ?risposta 4)
            then
            (printout t "Deve indicare se il bambino gioca con un fine strettamente legato alla funzione dell'oggetto (ad esempio, fa camminare la macchinina, lancia la palla), simbolico (es. fingere scene di vita quotidiana con i bambolotti) o basato sulla ricerca di effetti sensoriali (es. usare l'oggetto per produrre insistentemente un suono)." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 14) (num-risposta ?risposta)))
)

(defrule domanda-14-caso-2
    (fase (num 2))
    (ipotesi spettro-autistico)
    (conferma spettro-autistico)
    (not (gioco-oggetti ?gioco))
    =>
	(printout t "14. Il gioco con gli oggetti e':" crlf)
	(printout t "   1) Strettamente legato alla funzione dell'oggetto" crlf)
	(printout t "   2) Di tipo simbolico" crlf)
    (printout t "   3) Basato sulla ricerca di effetti sensoriali" crlf)
    (printout t "   4) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 3 ?risposta) FALSE)
		(if (eq ?risposta 4)
            then
            (printout t "Deve indicare se il bambino gioca con un fine strettamente legato alla funzione dell'oggetto (ad esempio, fa camminare la macchinina, lancia la palla), simbolico (es. fingere scene di vita quotidiana con i bambolotti) o basato sulla ricerca di effetti sensoriali (es. usare l'oggetto per produrre insistentemente un suono)." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 14) (num-risposta ?risposta)))
)

(defrule domanda-14-caso-3
    (fase (num 2))
    (ipotesi ritardo-mentale)
    (conferma ritardo-mentale)
    (not (gioco-oggetti ?gioco))
    =>
    (printout t "14. Il gioco con gli oggetti e':" crlf)
	(printout t "   1) Strettamente legato alla funzione dell'oggetto" crlf)
	(printout t "   2) Di tipo simbolico" crlf)
    (printout t "   3) Basato sulla ricerca di effetti sensoriali" crlf)
    (printout t "   4) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 3 ?risposta) FALSE)
		(if (eq ?risposta 4)
            then
            (printout t "Deve indicare se il bambino gioca con un fine strettamente legato alla funzione dell'oggetto (ad esempio, fa camminare la macchinina, lancia la palla), simbolico (es. fingere scene di vita quotidiana con i bambolotti) o basato sulla ricerca di effetti sensoriali (es. usare l'oggetto per produrre insistentemente un suono)." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 14) (num-risposta ?risposta)))
)

(defrule gioco-oggetti-legato-funzione
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 14) (num-risposta 1))
    =>
    (retract ?risp)
    (assert (gioco-oggetti legato-funzione))
)

(defrule gioco-oggetti-simbolico
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 14) (num-risposta 2))
    =>
    (retract ?risp)
    (assert (gioco-oggetti simbolico))
)

(defrule gioco-oggetti-sensoriale
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 14) (num-risposta 3))
    =>
    (retract ?risp)
    (assert (gioco-oggetti sensoriale))
)

(defrule domanda-17
    (fase (num 2))
    (ipotesi spettro-autistico)
    (conferma spettro-autistico)
    (not (movimenti-comportamenti-bizzarri ?movimenti))
    =>
    (printout t "17. Fa movimenti o ha comportamenti bizzarri?" crlf)
	(printout t "   1) Si" crlf)
	(printout t "   2) No" crlf)
    (printout t "   3) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 2 ?risposta) FALSE)
		(if (eq ?risposta 3)
            then
            (printout t "Deve indicare se il bambino ha comportamenti o fa movimenti bizzari, come sfarfallare, camminare in punta, guardare linee sui muri o sul pavimento o oggetti che girano." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 17) (num-risposta ?risposta)))
)

(defrule movimenti-comportamenti-bizzarri-si
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 17) (num-risposta 1))
    =>
    (retract ?risp)
    (assert (movimenti-comportamenti-bizzarri si))
)

(defrule movimenti-comportamenti-bizzarri-no
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 17) (num-risposta 2))
    =>
    (retract ?risp)
    (assert (movimenti-comportamenti-bizzarri no))
)

(defrule domanda-20
    (fase (num 2))
    (ipotesi spettro-autistico)
    (conferma spettro-autistico)
    (not (mangia ?mangia))
    =>
    (printout t "20. Mangia:" crlf)
	(printout t "   1) Regolarmente" crlf)
	(printout t "   2) Solo pochi cibi secondo un suo criterio" crlf)
    (printout t "   3) Solo cibi facili da masticare" crlf)
    (printout t "   4) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 3 ?risposta) FALSE)
		(if (eq ?risposta 4)
            then
            (printout t "Deve indicare se il bambino preferisce cibi di minore consistenza, cibi che non sono conditi, solo un tipo di portata o merende di una certa casa produttrice." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 20) (num-risposta ?risposta)))
)

(defrule mangia-regolarmente
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 20) (num-risposta 1))
    =>
    (retract ?risp)
    (assert (mangia regolarmente))
)

(defrule mangia-pochi-cibi
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 20) (num-risposta 2))
    =>
    (retract ?risp)
    (assert (mangia pochi-cibi))
)

(defrule mangia-cibi-facili-masticare
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 20) (num-risposta 3))
    =>
    (retract ?risp)
    (assert (mangia cibi-facili-masticare))
)

(defrule domanda-7
    (fase (num 2))
    (ipotesi ritardo-mentale)
    (not (cammina ?cammina))
    =>
    (printout t "7. Il bambino ha camminato da solo:" crlf)
	(printout t "   1) Entro i 16 mesi" crlf)
	(printout t "   2) Dopo i 16 mesi" crlf)
	(printout t "   3) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 2 ?risposta) FALSE)
		(if (eq ?risposta 3)
            then
            (printout t "Deve indicare quando il bambino ha fatto i primi passi senza aiuto e senza appoggiarsi." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 7) (num-risposta ?risposta)))
)

(defrule cammina-prima-16-mesi
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 7) (num-risposta 1))
    =>
    (retract ?risp)
    (assert (cammina prima-16-mesi))
)

(defrule cammina-dopo-16-mesi
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 7) (num-risposta 2))
    =>
    (retract ?risp)
    (assert (cammina dopo-16-mesi))
)

(defrule domanda-21
    (fase (num 2))
    (ipotesi ritardo-mentale)
    (not (autonomie ?autonomie))
    =>
    (printout t "21. Ha autonomie proporizionate all'eta'?" crlf)
	(printout t "   1) Si" crlf)
	(printout t "   2) No" crlf)
    (printout t "   3) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 2 ?risposta) FALSE)
		(if (eq ?risposta 3)
            then
            (printout t "Deve indicare se il bambino ha delle autonomie collegate alla sua eta', quali uso del pannolino, partecipazione alle pratiche di alimentazione, igiene e abbigliamento ecc." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 21) (num-risposta ?risposta)))
)

(defrule autonomie-si
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 21) (num-risposta 1))
    =>
    (retract ?risp)
    (assert (autonomie si))
)

(defrule autonomie-no
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 21) (num-risposta 2))
    =>
    (retract ?risp)
    (assert (autonomie no))
)

(defrule domanda-22
    (fase (num 2))
    (ipotesi ritardo-mentale)
    (not (partecipa-didattica ?partecipa))
    =>
	(printout t "22. Se scolarizzato, partecipa sufficientemente alle attivita' didattiche?" crlf)
	(printout t "   1) Si" crlf)
	(printout t "   2) No" crlf)
    (printout t "   3) Non e' scolarizzato" crlf)
    (printout t "   4) Non capisco la domanda" crlf)
	(bind ?risposta (read))
	(while (eq (check-input 3 ?risposta) FALSE)
		(if (eq ?risposta 4)
            then
            (printout t "Deve indicare se il bambino, se scolarizzato, e' interessato ai compiti scolastici e/o presta giusta attenzione e impegno." crlf)
            (printout t "Quale risposta vuole quindi fornire?" crlf)
            else
            (printout t "Fornire una risposta corretta." crlf)
        )
        (bind ?risposta (read))
	)
	(assert (domanda (num-domanda 22) (num-risposta ?risposta)))
)

(defrule partecipa-didattica-si
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 22) (num-risposta 1))
    =>
    (retract ?risp)
    (assert (partecipa-didattica si))
)

(defrule partecipa-didattica-no
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 22) (num-risposta 2))
    =>
    (retract ?risp)
    (assert (partecipa-didattica no))
)

(defrule partecipa-didattica-non-scolarizzato
    (declare (salience 1))
    (fase (num 2))
    ?risp <- (domanda (num-domanda 22) (num-risposta 3))
    =>
    (retract ?risp)
    (assert (partecipa-didattica non-scolarizzato))
)

(defrule ignoto-1-1
    (declare (salience 1))
    ?ipotesi <- (ipotesi fono-articolatorio)
    (altre-difficolta si)
    ?fase <- (fase (num ?n))
    (not (trovato no))
    =>
    (assert (trovato no))
    (retract ?ipotesi)
    (retract ?fase)
)

(defrule ignoto-1-2
    (declare (salience 1))
    ?ipotesi <- (ipotesi disfasia-espressiva)
    (altre-difficolta si)
    ?fase <- (fase (num ?n))
    (not (trovato no))
    =>
    (assert (trovato no))
    (retract ?ipotesi)
    (retract ?fase)
)

(defrule ignoto-1-3
    (declare (salience 1))
    ?ipotesi <- (ipotesi ritardo-semplice)
    (altre-difficolta si)
    ?fase <- (fase (num ?n))
    (not (trovato no))
    =>
    (assert (trovato no))
    (retract ?ipotesi)
    (retract ?fase)
)

(defrule ignoto-2
    (declare (salience 1))
    ?ipotesi <- (ipotesi spettro-autistico)
    ?conferma <- (conferma spettro-autistico)
    (or (esprime-desiderio gesti) (gioco-oggetti simbolico) (movimenti-comportamenti-bizzarri no) (resistenze-spavento-luoghi no) (mangia regolarmente) (mangia cibi-facili-masticare))
    ?fase <- (fase (num 2))
    =>
    (assert (trovato no))
    (retract ?conferma)
    (retract ?ipotesi)
    (retract ?fase)
)

(defrule ignoto-3
    (declare (salience 1))
    ?ipotesi <- (ipotesi ritardo-mentale)
    ?conferma <- (conferma ritardo-mentale)
    (or (iperattivo-caotico-poco-attento poco-niente) (gioco-oggetti simbolico) (gioco-oggetti sensoriale) (esprime-desiderio gesti) (esprime-desiderio mano-dell-altro))
    ?fase <- (fase (num 2))
    =>
    (assert (trovato no))
    (retract ?conferma)
    (retract ?ipotesi)
    (retract ?fase)
)

(defrule spettro-autistico-1
    (declare (salience 4))
    ?fase <- (fase (num 2))
    ?ipotesi <- (ipotesi spettro-autistico)
    ?conferma <- (conferma spettro-autistico)
    (or (esprime-desiderio indicando) (esprime-desiderio mano-dell-altro) (esprime-desiderio piangendo))
    (or (gioco-oggetti legato-funzione) (gioco-oggetti sensoriale))
    (movimenti-comportamenti-bizzarri si)
    (resistenze-spavento-luoghi si)
    (mangia pochi-cibi)
    ?ritardo <- (causa (risultato ritardo-mentale))
    ?mista <- (causa (risultato disfasia-mista))
    =>
    (retract ?fase)
    (retract ?ipotesi)
    (retract ?conferma)
    (retract ?ritardo)
    (retract ?mista)
    (assert (trovato si))
)

(defrule spettro-autistico-2
    (declare (salience 3))
    ?fase <- (fase (num 2))
    ?ipotesi <- (ipotesi spettro-autistico)
    ?conferma <- (conferma spettro-autistico)
    (or (esprime-desiderio indicando) (esprime-desiderio mano-dell-altro) (esprime-desiderio piangendo))
    (or (gioco-oggetti legato-funzione) (gioco-oggetti sensoriale))
    (movimenti-comportamenti-bizzarri si)
    (resistenze-spavento-luoghi si)
    (mangia pochi-cibi)
    ?ritardo <- (causa (risultato ritardo-mentale))
    =>
    (retract ?fase)
    (retract ?ipotesi)
    (retract ?conferma)
    (retract ?ritardo)
    (assert (trovato si))
)

(defrule spettro-autistico-3
    (declare (salience 3))
    ?fase <- (fase (num 2))
    ?ipotesi <- (ipotesi spettro-autistico)
    ?conferma <- (conferma spettro-autistico)
    (or (esprime-desiderio indicando) (esprime-desiderio mano-dell-altro) (esprime-desiderio piangendo))
    (or (gioco-oggetti legato-funzione) (gioco-oggetti sensoriale))
    (movimenti-comportamenti-bizzarri si)
    (resistenze-spavento-luoghi si)
    (mangia pochi-cibi)
    ?mista <- (causa (risultato disfasia-mista))
    =>
    (retract ?fase)
    (retract ?ipotesi)
    (retract ?conferma)
    (retract ?mista)
    (assert (trovato si))
)

(defrule spettro-autistico-4
    (declare (salience 2))
    ?fase <- (fase (num 2))
    ?ipotesi <- (ipotesi spettro-autistico)
    ?conferma <- (conferma spettro-autistico)
    (or (esprime-desiderio indicando) (esprime-desiderio mano-dell-altro) (esprime-desiderio piangendo))
    (or (gioco-oggetti legato-funzione) (gioco-oggetti sensoriale))
    (movimenti-comportamenti-bizzarri si)
    (resistenze-spavento-luoghi si)
    (mangia pochi-cibi)
    =>
    (retract ?fase)
    (retract ?ipotesi)
    (retract ?conferma)
    (assert (trovato si))
)

(defrule ritardo-semplice
    ?ipotesi <- (ipotesi ritardo-semplice)
    (altre-difficolta no)
    ?autismo <- (causa (risultato spettro-autistico))
    ?ritardo <- (causa (risultato ritardo-mentale))
    ?mista <- (causa (risultato disfasia-mista))
    ?fase <- (fase (num ?n))
    =>
    (retract ?autismo)
    (retract ?ritardo)
    (retract ?mista)
    (retract ?ipotesi)
    (retract ?fase)
    (assert (trovato si))
)

(defrule disfasia-espressiva
    ?ipotesi <- (ipotesi disfasia-espressiva)
    (altre-difficolta no)
    ?autismo <- (causa (risultato spettro-autistico))
    ?ritardo <- (causa (risultato ritardo-mentale))
    ?mista <- (causa (risultato disfasia-mista))
    ?fase <- (fase (num ?n))
    =>
    (retract ?autismo)
    (retract ?ritardo)
    (retract ?mista)
    (retract ?ipotesi)
    (retract ?fase)
    (assert (trovato si))
)

(defrule fono-articolatorio
    ?ipotesi <- (ipotesi fono-articolatorio)
    (altre-difficolta no)
    ?autismo <- (causa (risultato spettro-autistico))
    ?ritardo <- (causa (risultato ritardo-mentale))
    ?mista <- (causa (risultato disfasia-mista))
    ?fase <- (fase (num ?n))
    =>
    (retract ?autismo)
    (retract ?ritardo)
    (retract ?mista)
    (retract ?ipotesi)
    (retract ?fase)
    (assert (trovato si))
)

(defrule sordita-ipoacusia
    ?ipotesi <- (ipotesi sordita-ipoacusia)
    (reagisce-richiami-se-guarda si)
    (or (reagisce-suoni no) (reagisce-suoni non-sempre))
    ?autismo <- (causa (risultato spettro-autistico))
    ?ritardo <- (causa (risultato ritardo-mentale))
    ?mista <- (causa (risultato disfasia-mista))
    ?fase <- (fase (num ?n))
    =>
    (retract ?autismo)
    (retract ?ritardo)
    (retract ?mista)
    (retract ?ipotesi)
    (retract ?fase)
    (assert (trovato si))
)

(defrule disfasia-mista-1
    (declare (salience 4))
    (causa (risultato disfasia-mista))
    ?ipotesi <- (ipotesi disfasia-mista)
    (or (esprime-desiderio indicando) (esprime-desiderio gesti))
    (abitudinario no)
    (resistenze-spavento-luoghi no)
    (or (gioco-oggetti legato-funzione) (gioco-oggetti simbolico))
    ?ritardo <- (causa (risultato ritardo-mentale))
    ?autismo <- (causa (risultato spettro-autistico))
    ?fase <- (fase (num 2))
    =>
    (retract ?ipotesi)
    (retract ?fase)
    (retract ?ritardo)
    (retract ?autismo)
    (assert (trovato si))
)

(defrule disfasia-mista-2
    (declare (salience 3))
    (causa (risultato disfasia-mista))
    ?ipotesi <- (ipotesi disfasia-mista)
    (or (esprime-desiderio indicando) (esprime-desiderio gesti))
    (abitudinario no)
    (resistenze-spavento-luoghi no)
    (or (gioco-oggetti legato-funzione) (gioco-oggetti simbolico))
    ?ritardo <- (causa (risultato ritardo-mentale))
    ?fase <- (fase (num 2))
    =>
    (retract ?ipotesi)
    (retract ?fase)
    (retract ?ritardo)
    (assert (trovato si))
)

(defrule disfasia-mista-3
    (declare (salience 3))
    (causa (risultato disfasia-mista))
    ?ipotesi <- (ipotesi disfasia-mista)
    (or (esprime-desiderio indicando) (esprime-desiderio gesti))
    (abitudinario no)
    (resistenze-spavento-luoghi no)
    (or (gioco-oggetti legato-funzione) (gioco-oggetti simbolico))
    ?autismo <- (causa (risultato spettro-autistico))
    ?fase <- (fase (num 2))
    =>
    (retract ?ipotesi)
    (retract ?fase)
    (retract ?autismo)
    (assert (trovato si))
)

(defrule disfasia-mista-4
    (declare (salience 2))
    (causa (risultato disfasia-mista))
    ?ipotesi <- (ipotesi disfasia-mista)
    (or (esprime-desiderio indicando) (esprime-desiderio gesti))
    (abitudinario no)
    (resistenze-spavento-luoghi no)
    (or (gioco-oggetti legato-funzione) (gioco-oggetti simbolico))
    ?fase <- (fase (num 2))
    =>
    (retract ?ipotesi)
    (retract ?fase)
    (assert (trovato si))
)

(defrule conferma-no-disfasia-mista
    (declare (salience 1))
    (fase (num 2))
    ?mista <- (causa (risultato disfasia-mista))
    ?ipotesi <- (ipotesi disfasia-mista)
    (or (abitudinario si) (resistenze-spavento-luoghi si) (gioco-oggetti sensoriale) (esprime-desiderio mano-dell-altro) (esprime-desiderio piangendo))
    =>
    (retract ?mista)
    (retract ?ipotesi)
)

(defrule accertamento-spettro-autistico
    (declare (salience 1))
    (causa (risultato spettro-autistico))
    (ipotesi spettro-autistico)
    (or (risponde-nome no) (risponde-nome non-sempre))
    (guarda-occhi poco-niente)
    (interessato-gioco-altri poco-niente)
    (accetta-modifica-schemi poco-niente)
    (abitudinario si)
    (fase (num 2))
    =>
    (assert (conferma spettro-autistico))
)

(defrule conferma-no-spettro-autistico
    (declare (salience 1))
    (fase (num 2))
    ?autismo <- (causa (risultato spettro-autistico))
    ?ipotesi <- (ipotesi spettro-autistico)
    (or (risponde-nome si) (guarda-occhi si) (interessato-gioco-altri si) (accetta-modifica-schemi si) (abitudinario no))
    =>
    (retract ?autismo)
    (retract ?ipotesi)
)

(defrule accertamento-ritardo-mentale
    (declare (salience 1))
    (causa (risultato ritardo-mentale))
    (ipotesi ritardo-mentale)
    (cammina dopo-16-mesi)
    (autonomie no)
    (or (partecipa-didattica no) (partecipa-didattica non-scolarizzato))
    (fase (num 2))
    =>
    (assert (conferma ritardo-mentale))
)

(defrule conferma-no-ritardo-mentale
    (declare (salience 1))
    (fase (num 2))
    ?ritardo <- (causa (risultato ritardo-mentale))
    ?ipotesi <- (ipotesi ritardo-mentale)
    (or (cammina prima-16-mesi) (autonomie si) (partecipa-didattica si))
    =>
    (retract ?ritardo)
    (retract ?ipotesi)
)

(defrule ritardo-mentale-1
    (declare (salience 4))
    ?fase <- (fase (num 2))
    ?ipotesi <- (ipotesi ritardo-mentale)
    ?conferma <- (conferma ritardo-mentale)
    (iperattivo-caotico-poco-attento si)
    (gioco-oggetti legato-funzione)
    (or (esprime-desiderio indicando) (esprime-desiderio piangendo))
    ?autismo <- (causa (risultato spettro-autistico))
    ?mista <- (causa (risultato disfasia-mista))
    =>
    (retract ?fase)
    (retract ?ipotesi)
    (retract ?conferma)
    (retract ?autismo)
    (retract ?mista)
    (assert (trovato si))
)

(defrule ritardo-mentale-2
    (declare (salience 3))
    ?fase <- (fase (num 2))
    ?ipotesi <- (ipotesi ritardo-mentale)
    ?conferma <- (conferma ritardo-mentale)
    (iperattivo-caotico-poco-attento si)
    (gioco-oggetti legato-funzione)
    (or (esprime-desiderio indicando) (esprime-desiderio piangendo))
    ?autismo <- (causa (risultato spettro-autistico))
    =>
    (retract ?fase)
    (retract ?ipotesi)
    (retract ?conferma)
    (retract ?autismo)
    (assert (trovato si))
)

(defrule ritardo-mentale-3
    (declare (salience 3))
    ?fase <- (fase (num 2))
    ?ipotesi <- (ipotesi ritardo-mentale)
    ?conferma <- (conferma ritardo-mentale)
    (iperattivo-caotico-poco-attento si)
    (gioco-oggetti legato-funzione)
    (or (esprime-desiderio indicando) (esprime-desiderio piangendo))
    ?mista <- (causa (risultato disfasia-mista))
    =>
    (retract ?fase)
    (retract ?ipotesi)
    (retract ?conferma)
    (retract ?mista)
    (assert (trovato si))
)

(defrule ritardo-mentale-4
    (declare (salience 2))
    ?fase <- (fase (num 2))
    ?ipotesi <- (ipotesi ritardo-mentale)
    ?conferma <- (conferma ritardo-mentale)
    (iperattivo-caotico-poco-attento si)
    (gioco-oggetti legato-funzione)
    (or (esprime-desiderio indicando) (esprime-desiderio piangendo))
    =>
    (retract ?fase)
    (retract ?ipotesi)
    (retract ?conferma)
    (assert (trovato si))
)

(defrule diagnosi-disfasia-mista
    ?trovato <- (trovato si)
    (causa (risultato disfasia-mista))
    =>
    (retract ?trovato)
    (printout t "Il ritardo del linguaggio e' molto probabilmente causato da disfasia mista. E' necessario un consulto Neuropsichiatrico Infantile. La valutazione Neuropsichiatrica potra' confermare il sospetto attraverso l'osservazione diretta e la somministrazione di una batteria di test per la valutazione del linguaggio e di un test intellettivo." crlf)
)

(defrule diagnosi-spettro-autistico
    ?trovato <- (trovato si)
    (causa (risultato spettro-autistico))
    =>
    (retract ?trovato)
    (printout t "Le caratteristiche di questo ritardo di linguaggio suggeriscono un disturbo dello spettro autistico. E' necessario, con particolare urgenza, un consulto Neuropsichiatrico Infantile. La valutazione Neuropsichiatrica potra' confermare il sospetto attraverso l'osservazione diretta e la somministrazione del test ADOS." crlf)
)

(defrule diagnosi-ritardo-mentale
    ?trovato <- (trovato si)
    (causa (risultato ritardo-mentale))
    =>
    (retract ?trovato)
    (printout t "Il ritardo del linguaggio e' molto probabilmente causato da ritardo mentale. E' necessario, con particolare urgenza, un consulto Neuropsichiatrico Infantile. La valutazione Neuropsichiatrica potra' confermare il sospetto attraverso l'osservazione diretta e la somministrazione di un test intellettivo." crlf)
)

(defrule diagnosi-sordita-ipoacusia
    ?trovato <- (trovato si)
    (causa (risultato sordita-ipoacusia))
    =>
    (retract ?trovato)
    (printout t "Il ritardo del linguaggio e' molto probabilmente da sordita' o ipoacusia. Potrebbe essere indispensabile una visita audiologica." crlf)
)

(defrule diagnosi-ritardo-semplice
    ?trovato <- (trovato si)
    (causa (risultato ritardo-semplice))
    =>
    (retract ?trovato)
    (printout t "Il ritardo del linguaggio e' molto probabilmente causato da ritardo semplice del linguaggio. Non e' necessario un consulto specialistico ma bisognerebbe rivalutare la situazione entro qualche mese." crlf)
)

(defrule diagnosi-fono-articolatorio
    ?trovato <- (trovato si)
    (causa (risultato fono-articolatorio))
    =>
    (retract ?trovato)
    (printout t "Il ritardo del linguaggio e' molto probabilmente causato da un disturbo fono articolatorio. Se il bambino ha meno di tre anni e/o i suoni alterati sono pochi bisognerebbe rivalutare la situazione fra qualche mese. In alternativa si potrebbe indirizzare la famiglia verso un percorso di tipo logopedico." crlf)
)

(defrule diagnosi-disfasia-espressiva
    ?trovato <- (trovato si)
    (causa (risultato disfasia-espressiva))
    =>
    (retract ?trovato)
    (printout t "Il ritardo del linguaggio e' molto probabilmente causato da disfasia espressiva. E' necessario un consulto Neuropsichiatrico Infantile. La valutazione Neuropsichiatrica potra' confermare il sospetto attraverso l'osservazione diretta e la somministrazione di una batteria di test per la valutazione del linguaggio e di un test intellettivo." crlf)
)

(defrule diagnosi-ignoto
    (declare (salience 1))
    (trovato no)
    =>
    (printout t "Non e' stato possibile trovare una causa precisa, e' indispensabile quindi un consulto neuropsichiatrico infantile, tuttavia le cause ancora possibili sono:" crlf)
)

(defrule diagnosi-ignoto-ritardo-semplice
    (trovato no)
    (causa (risultato ritardo-semplice))
    =>
    (printout t "   - ritardo semplice del linguaggio" crlf)
)

(defrule diagnosi-ignoto-sordita-ipoacusia
    (trovato no)
    (causa (risultato sordita-ipoacusia))
    =>
    (printout t "   - sordita' o ipoacusia" crlf)
)

(defrule diagnosi-ignoto-disfasia-espressiva
    (trovato no)
    (causa (risultato disfasia-espressiva))
    =>
    (printout t "   - disfasia espressiva" crlf)
)

(defrule diagnosi-ignoto-disfasia-mista
    (trovato no)
    (causa (risultato disfasia-mista))
    =>
    (printout t "   - disfasia mista" crlf)
)

(defrule diagnosi-ignoto-ritardo-mentale
    (trovato no)
    (causa (risultato ritardo-mentale))
    =>
    (printout t "   - ritardo mentale" crlf)
)

(defrule diagnosi-ignoto-spettro-autistico
    (trovato no)
    (causa (risultato spettro-autistico))
    =>
    (printout t "   - disturbo dello spettro autistico" crlf)
)

(defrule diagnosi-ignoto-fono-articolatorio
    (trovato no)
    (causa (risultato fono-articolatorio))
    =>
    (printout t "   - disturbo fono-articolatorio" crlf)
)

(defrule niente
    (not (causa (risultato spettro-autistico)))
    (not (causa (risultato ritardo-mentale)))
    (not (causa (risultato disfasia-mista)))
    (not (causa (risultato fono-articolatorio)))
    (not (causa (risultato disfasia-espressiva)))
    (not (causa (risultato ritardo-semplice)))
    (not (causa (risultato sordita-ipoacusia)))
    =>
    (printout t "Non e' stato possibile trovare una causa precisa, e' indispensabile quindi un consulto neuropsichiatrico infantile che potra' chiarire le cause del ritardo del linguaggio." crlf)
)

(defrule trovato-no
    (declare (salience -1))
    ?trovato <- (trovato no)
    =>
    (retract ?trovato)
)